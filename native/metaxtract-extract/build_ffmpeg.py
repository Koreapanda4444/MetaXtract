from __future__ import annotations

import argparse
import contextlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

DECODERS = (
    "h264,hevc,av1,mpeg4,mpeg1video,mpeg2video,vp8,vp9,mjpeg,png,rawvideo,"
    "mp3,mp3float,aac,ac3,eac3,flac,vorbis,opus,alac,ape,wavpack,"
    "pcm_s16le,pcm_s16be,pcm_u8,pcm_s24le,pcm_s24be,pcm_s32le,pcm_s32be,"
    "pcm_f32le,pcm_f32be,pcm_f64le,pcm_f64be"
)

@contextlib.contextmanager
def build_lock(path: Path):
    with path.open("a+b") as handle:
        if os.name == "nt":
            import msvcrt
            handle.seek(0)
            handle.write(b"0")
            handle.flush()
            while True:
                handle.seek(0)
                try:
                    msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
                    break
                except OSError:
                    time.sleep(0.2)
        else:
            import fcntl
            fcntl.flock(handle, fcntl.LOCK_EX)
        try:
            yield
        finally:
            if os.name == "nt":
                handle.seek(0)
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                fcntl.flock(handle, fcntl.LOCK_UN)

def windows_environment() -> dict[str, str]:
    env = os.environ.copy()
    compiler = shutil.which("cl", path=env.get("PATH"))
    if compiler:
        env["PATH"] = str(Path(compiler).parent) + os.pathsep + env.get("PATH", "")
        return env
    vswhere = Path(os.environ.get("ProgramFiles(x86)", "C:/Program Files (x86)")) / "Microsoft Visual Studio/Installer/vswhere.exe"
    installation = subprocess.check_output(
        [str(vswhere), "-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"],
        text=True,
    ).strip()
    batch = Path(installation) / "VC/Auxiliary/Build/vcvars64.bat"
    output = subprocess.check_output(
        ["cmd", "/d", "/s", "/c", f'"{batch}" >nul && set'], text=True
    )
    for line in output.splitlines():
        if "=" in line and not line.startswith("="):
            key, value = line.split("=", 1)
            env[key.upper()] = value
    compiler = shutil.which("cl", path=env.get("PATH"))
    if not compiler:
        raise RuntimeError("MSVC compiler was not found after vcvars64")
    env["PATH"] = str(Path(compiler).parent) + os.pathsep + env.get("PATH", "")
    return env

def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--build", type=Path, required=True)
    parser.add_argument("--zlib", type=Path, required=True)
    parser.add_argument("--zlib-include", type=Path, required=True)
    parser.add_argument("--cc", required=True)
    parser.add_argument("--sanitize", default="")
    parser.add_argument("--static-crt", default="OFF")
    args = parser.parse_args()
    incoming, build = args.source.resolve(), args.build.resolve()
    build.mkdir(parents=True, exist_ok=True)
    with build_lock(build / "build.lock"):
        # The cache key includes the reviewed source revision. Keep its source path
        # stable across Cargo debug/release, conformance and standalone builds.
        source = build / "source"
        if not (source / "configure").is_file():
            if source.exists():
                shutil.rmtree(source)
            shutil.copytree(incoming, source, ignore=shutil.ignore_patterns(".git"))
        prefix = build / "install"
        pinned = build / "pinned-zlib"
        pinned.mkdir(exist_ok=True)
        archive = pinned / ("zlib.lib" if os.name == "nt" else "libz.a")
        if not archive.is_file() or archive.read_bytes() != args.zlib.read_bytes():
            shutil.copy2(args.zlib, archive)
        env = windows_environment() if os.name == "nt" else os.environ.copy()
        msys = Path(env.get("METAXTRACT_MSYS_ROOT", "C:/msys64")) / "usr/bin"
        if os.name == "nt":
            shell, make = str(msys / "bash.exe"), str(msys / "make.exe")
            if not Path(shell).is_file() or not Path(make).is_file():
                raise RuntimeError("MSVC FFmpeg builds require MSYS2 bash and GNU make (see README)")
            env["PATH"] = env.get("PATH", "") + os.pathsep + str(msys)
            env["MSYS2_ARG_CONV_EXCL"] = "*"
        else:
            shell = shutil.which("bash") or "/bin/sh"
            make = shutil.which("gmake") or shutil.which("make")
            if not make:
                raise RuntimeError("FFmpeg builds require GNU make")
        flags = [
            "--disable-programs", "--disable-doc", "--disable-network", "--disable-autodetect",
            "--disable-gpl", "--disable-nonfree", "--disable-static", "--enable-shared", "--pkg-config=false",
            "--enable-pic", "--disable-x86asm", "--disable-asm", "--disable-protocols",
            "--disable-encoders", "--disable-muxers", "--disable-filters", "--disable-avfilter",
            "--disable-indevs", "--disable-outdevs", "--disable-avdevice", "--disable-hwaccels",
            "--disable-decoders", "--enable-decoder=" + DECODERS, "--enable-zlib",
            "--prefix=" + prefix.as_posix(), "--shlibdir=" + (prefix / "lib").as_posix(),
        ]
        include = pinned / "include"
        include.mkdir(exist_ok=True)
        for name in ("zlib.h", "zconf.h"):
            incoming_header = args.zlib_include / name
            cached_header = include / name
            if not cached_header.is_file() or cached_header.read_bytes() != incoming_header.read_bytes():
                shutil.copy2(incoming_header, cached_header)
        cflags = '-I"' + include.as_posix() + '"'
        ldflags = '-L"' + pinned.as_posix() + '"'
        if os.name == "nt":
            flags += ["--toolchain=msvc", "--target-os=win64", "--arch=x86_64", "--cc=cl", "--ld=link"]
            cflags += " -MT" if args.static_crt == "ON" else " -MD"
            # configure does not retroactively translate its initial LDFLAGS.
            ldflags = '/LIBPATH:"' + pinned.as_posix() + '"'
        else:
            flags += ["--cc=" + args.cc]
            if sys.platform == "darwin":
                flags += ["--install-name-dir=@rpath"]
        if args.sanitize:
            cflags += " -fsanitize=address,undefined -fno-omit-frame-pointer"
            ldflags += " -fsanitize=address,undefined"
        flags += ["--extra-cflags=" + cflags, "--extra-ldflags=" + ldflags]
        configuration = {"source": str(source), "flags": flags, "compiler": args.cc}
        if sys.platform.startswith("linux"):
            configuration["shared_library_runpath"] = "$ORIGIN"
        config_file = build / "reviewed-configuration.json"
        if not (build / "ffbuild/config.mak").is_file() or not config_file.is_file() or json.loads(config_file.read_text()) != configuration:
            with (build / "configure.log").open("w") as log:
                try:
                    subprocess.run([shell, source.as_posix() + "/configure", *flags], cwd=build, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
                except subprocess.CalledProcessError:
                    print((build / "configure.log").read_text(errors="replace")[-12000:], file=sys.stderr)
                    if (build / "ffbuild/config.log").is_file():
                        configuration_log = (build / "ffbuild/config.log").read_text(errors="replace")
                        for token in ("check_lib zlib", "zlibVersion", "zlib.lib"):
                            at = configuration_log.find(token)
                            if at >= 0:
                                print(configuration_log[max(0, at - 800):at + 2600], file=sys.stderr)
                        print(configuration_log[-16000:], file=sys.stderr)
                    raise
            config_file.write_text(json.dumps(configuration, indent=2) + "\n")
        if sys.platform.startswith("linux"):
            # GNU make consumes $; the shell's single quotes preserve $ORIGIN.
            # Add this after configure's eval-based flag handling.
            make_config = build / "ffbuild/config.mak"
            override = "\n# MetaXtract shared-library sibling lookup\nLDFLAGS += -Wl,-rpath,'$$$$ORIGIN'\n"
            contents = make_config.read_text()
            if override not in contents:
                make_config.write_text(contents + override)
            configuration["shared_library_runpath"] = "$ORIGIN"
        config = (build / "config.h").read_text()
        for item in ("GPL", "NONFREE", "NETWORK"):
            if f"#define CONFIG_{item} 0" not in config:
                raise RuntimeError(f"Forbidden FFmpeg configuration: {item}")
        with (build / "build.log").open("w") as log:
            try:
                subprocess.run([make, "-j", "3"], cwd=build, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
                subprocess.run([make, "install"], cwd=build, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
            except subprocess.CalledProcessError:
                print((build / "build.log").read_text(errors="replace")[-12000:], file=sys.stderr)
                raise
        if sys.platform.startswith("linux"):
            for library in (prefix / "lib").glob("lib*.so.*"):
                if library.is_symlink():
                    continue
                dynamic = subprocess.check_output(["readelf", "-d", str(library)], text=True)
                if "$ORIGIN" not in dynamic:
                    raise RuntimeError("FFmpeg shared library is missing its relative runtime search path: " + str(library) + "\n" + dynamic)
        (build / "complete.json").write_text(json.dumps(configuration) + "\n")

if __name__ == "__main__":
    main()
