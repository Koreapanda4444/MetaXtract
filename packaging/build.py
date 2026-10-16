"""Assemble native-host release archives with no build tools in the runtime."""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import plistlib
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile
from pathlib import Path

import runtime

ROOT = Path(__file__).resolve().parents[1]
PROGRAMS = {"cli": "metaxtract", "gui": "metaxtract-gui", "worker": "metaxtract-worker"}


def sha256(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def build() -> tuple[dict[str, Path], Path]:
    command = ["cargo", "build", "--workspace", "--release", "--locked", "--message-format=json"]
    process = subprocess.Popen(command, cwd=ROOT, stdout=subprocess.PIPE, text=True, encoding="utf-8")
    programs, native = {}, None
    assert process.stdout is not None
    for line in process.stdout:
        message = json.loads(line)
        if message["reason"] == "compiler-message":
            print(message["message"].get("rendered", ""), file=sys.stderr, end="")
        if message["reason"] == "build-script-executed" and "metaxtract-native" in message["package_id"]:
            for linked in message["linked_paths"]:
                value = linked.removeprefix("native=")
                candidate = Path(value).parent
                if (candidate / "share/metaxtract/licenses").is_dir():
                    native = candidate
        if message["reason"] == "compiler-artifact" and message.get("executable"):
            name = message["target"]["name"]
            if name in PROGRAMS.values():
                programs[name] = Path(message["executable"])
    if process.wait():
        raise ValueError("release build failed")
    if set(programs) != set(PROGRAMS.values()) or native is None:
        raise ValueError("Cargo did not identify all three binaries and the native runtime")
    return programs, native


def copy_licenses(root: Path, native: Path) -> None:
    shutil.copytree(native / "share/metaxtract/licenses", root / "licenses/native")
    print("Collecting Rust dependency notices", file=sys.stderr, flush=True)
    metadata = json.loads(runtime.output("cargo", "metadata", "--locked", "--format-version", "1"))
    inventory = []
    for package in metadata["packages"]:
        if not package["source"]:
            continue
        source = Path(package["manifest_path"]).parent
        destination = root / "licenses/rust" / (package["name"] + "-" + package["version"])
        destination.mkdir(parents=True)
        for path in source.rglob("*"):
            name = path.name.lower()
            if path.is_file() and (name.startswith(("license", "licence", "unlicense", "copying", "notice", "copyright"))
                                   or any(word in name for word in ("license", "licence")) and path.suffix.lower() in (".txt", ".md")
                                   or name in ("ofl.txt", "ufl.txt")):
                relative = path.relative_to(source)
                target = destination / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(path, target)
        inventory.append({"name": package["name"], "version": package["version"],
                          "license": package["license"], "source": package["source"]})
    (root / "licenses/rust-dependencies.json").write_text(json.dumps(inventory, indent=2) + "\n", encoding="utf-8")
    for name in ("third-party.lock.toml", "Cargo.lock", "dependency-policy.toml"):
        shutil.copyfile(ROOT / name, root / "licenses" / name)


def mac_bundle(root: Path) -> tuple[Path, Path]:
    contents = root / "MetaXtract.app/Contents"
    binaries, libraries = contents / "MacOS", contents / "Frameworks"
    resources = contents / "Resources"
    for path in (binaries, libraries, resources):
        path.mkdir(parents=True)
    icon = (ROOT / "crates/metaxtract-gui/assets/icon.png").read_bytes()
    width, height = struct.unpack_from(">II", icon, 16)
    kind = {128: b"ic07", 256: b"ic08", 512: b"ic09", 1024: b"ic10"}.get(width)
    if width != height or kind is None:
        raise ValueError("desktop icon must have a standard square ICNS size")
    item = kind + struct.pack(">I", len(icon) + 8) + icon
    (resources / "MetaXtract.icns").write_bytes(b"icns" + struct.pack(">I", len(item) + 8) + item)
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    with (contents / "Info.plist").open("wb") as stream:
        plistlib.dump({"CFBundleIdentifier": "org.metaxtract.desktop", "CFBundleName": "MetaXtract",
                      "CFBundleExecutable": "metaxtract-gui", "CFBundlePackageType": "APPL",
                      "CFBundleShortVersionString": version, "CFBundleVersion": version,
                      "CFBundleIconFile": "MetaXtract.icns", "NSHighResolutionCapable": True}, stream)
    return binaries, libraries


def assemble(root: Path, programs: dict[str, Path], native: Path) -> dict:
    binaries, libraries = (mac_bundle(root) if sys.platform == "darwin"
                           else (root, root if os.name == "nt" else root / "lib"))
    libraries.mkdir(parents=True, exist_ok=True)
    paths, executables = [], {}
    for role, name in PROGRAMS.items():
        source = programs[name]
        destination = binaries / source.name
        shutil.copy2(source, destination)
        paths.append(destination)
        executables[role] = destination.relative_to(root).as_posix()
    pattern = "*.dll" if os.name == "nt" else "*.dylib" if sys.platform == "darwin" else "*.so*"
    copied = set()
    for source in sorted((native / "lib").glob(pattern)):
        name = runtime.library_name(source)
        if name in ("", ".", "..") or "/" in name or "\\" in name:
            raise ValueError(f"invalid runtime library name: {name}")
        destination = libraries / name
        if name in copied:
            if sha256(source) != sha256(destination):
                raise ValueError(f"conflicting runtime library: {name}")
            continue
        shutil.copy2(source, destination)
        copied.add(name)
        paths.append(destination)
    for name in ("mx_libraw", "tag", "avformat", "avcodec", "avutil", "swscale", "swresample"):
        if not any(name in p.lower() for p in copied):
            raise ValueError(f"required bundled library is missing: {name}")
    copy_licenses(root, native)
    if os.name == "nt":
        redist = runtime.windows_crt()
        while True:
            bundled = {p.name.lower() for p in paths}
            needed = {name for path in paths for name in runtime.imports(path)
                      if name.lower() not in bundled and not runtime.system_library(name)}
            if not needed:
                break
            for name in sorted(needed):
                source = redist / name
                if Path(name).name != name or not source.is_file():
                    raise ValueError(f"dependency is outside the approved CRT: {name}")
                destination = binaries / source.name
                shutil.copy2(source, destination)
                paths.append(destination)
        (root / "licenses/Microsoft-runtime.txt").write_text(
            "Microsoft Visual C++ Runtime / Microsoft Visual C++ 런타임\n"
            "Unmodified app-local REDIST files from / 수정하지 않은 앱 로컬 REDIST 파일의 출처:\n"
            + str(redist) + "\nhttps://learn.microsoft.com/visualstudio/releases/2026/redistribution\n"
            "https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files\n", encoding="utf-8")
    for name in ("README.md", "README_KR.md"):
        source = "packaging.md" if name == "README.md" else "packaging_KR.md"
        text = (ROOT / "docs" / source).read_text(encoding="utf-8")
        (root / name).write_text(text.replace("(packaging.md)", "(README.md)").replace(
            "(packaging_KR.md)", "(README_KR.md)"), encoding="utf-8")
    checked = runtime.inspect(root, paths)
    if sys.platform == "darwin":
        info_path = root / "MetaXtract.app/Contents/Info.plist"
        with info_path.open("rb") as stream:
            info = plistlib.load(stream)
        if checked["macos_minimum"]:
            info["LSMinimumSystemVersion"] = checked["macos_minimum"]
        with info_path.open("wb") as stream:
            plistlib.dump(info, stream)
        for path in paths:
            subprocess.run(["codesign", "--force", "--sign", "-", str(path)], check=True)
        subprocess.run(["codesign", "--force", "--sign", "-", str(root / "MetaXtract.app")], check=True)
        subprocess.run(["codesign", "--verify", "--deep", "--strict", str(root / "MetaXtract.app")], check=True)
    return {"executables": executables, "runtime": checked}


def sources(destination: Path, revision: str, dirty: bool) -> list[dict]:
    dependencies = tomllib.loads((ROOT / "third-party.lock.toml").read_text())["dependency"]
    included = []
    with tarfile.open(destination, "w:gz") as archive:
        tracked = subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT).decode().split("\0")
        # Include only our new build files for an explicitly marked local dirty build.
        if dirty:
            tracked += [str(p.relative_to(ROOT)) for p in (ROOT / "packaging").glob("*.py")]
            tracked += ["native/runtime.rs", "crates/metaxtract-cli/build.rs", "crates/metaxtract-gui/build.rs",
                        "docs/packaging.md", "docs/packaging_KR.md"]
        for name in sorted(set(tracked) - {""}):
            path = ROOT / name
            if path.is_file():
                archive.add(path, "MetaXtract-sources/application/" + name, recursive=False)
        for dependency in dependencies:
            source = ROOT / "target/native-sources" / (dependency["name"].lower() + "-" + dependency["revision"])
            if not source.is_dir():
                continue  # Planned, unused dependencies are not distributed libraries.
            print("Archiving native source: " + dependency["name"], file=sys.stderr, flush=True)
            process = subprocess.Popen(["git", "-C", str(source), "archive", dependency["revision"]], stdout=subprocess.PIPE)
            assert process.stdout is not None
            with tarfile.open(fileobj=process.stdout, mode="r|") as incoming:
                for item in incoming:
                    stream = incoming.extractfile(item) if item.isfile() else None
                    prefix = "MetaXtract-sources/upstream/" + dependency["name"] + "/"
                    item.name = prefix + item.name
                    if item.islnk():
                        item.linkname = prefix + item.linkname
                    archive.addfile(item, stream)
            # Git may write another complete zero-filled tar block after the first
            # EOF header. Drain it before waiting: Windows pipes can be smaller
            # than that block, leaving the producer blocked otherwise.
            while process.stdout.read(65536):
                pass
            process.stdout.close()
            if process.wait(timeout=60):
                raise ValueError("could not archive reviewed dependency source")
            included.append(dependency)
        data = json.dumps({"git_commit": revision, "source_dirty": dirty, "native_sources": included}, indent=2).encode()
        info = tarfile.TarInfo("MetaXtract-sources/provenance.json")
        info.size = len(data)
        archive.addfile(info, io.BytesIO(data))
        toolchain = "# Reviewed native sources / 검토한 네이티브 원본\n"
        for dependency in included:
            toolchain += 'set(FETCHCONTENT_SOURCE_DIR_' + dependency["name"].upper() + ' "${CMAKE_CURRENT_LIST_DIR}/upstream/' + dependency["name"] + '" CACHE PATH "Bundled source" FORCE)\n'
        data = toolchain.encode()
        info = tarfile.TarInfo("MetaXtract-sources/upstream-sources.cmake")
        info.size = len(data)
        archive.addfile(info, io.BytesIO(data))
    return included


def source_archive_fixture() -> None:
    """Exercise Git's extra tar trailer block through the real source packer."""
    global ROOT
    original = ROOT
    with tempfile.TemporaryDirectory(prefix="metaxtract-source-pipe-") as temporary:
        base = Path(temporary)
        upstream, app, hooks = base / "upstream", base / "application", base / "hooks"
        for path in (upstream, app, hooks):
            path.mkdir()

        def git(repo: Path, *args: str) -> str:
            return subprocess.check_output(["git", "-C", str(repo), *args], text=True,
                                           encoding="utf-8", stderr=subprocess.STDOUT, timeout=30).strip()

        def commit(repo: Path) -> str:
            git(repo, "add", ".")
            git(repo, "-c", "user.name=Archive fixture", "-c", "user.email=fixture@example.invalid",
                "-c", "commit.gpgsign=false", "-c", "core.hooksPath=" + str(hooks),
                "commit", "-qm", "source pipe fixture")
            return git(repo, "rev-parse", "HEAD")

        git(upstream, "init", "-q")
        # The commit pax header, file header and 8192-byte payload leave only
        # one EOF record in Git's current block, forcing another full block.
        payload = b"P" * 8192
        (upstream / "payload.bin").write_bytes(payload)
        revision = commit(upstream)
        git(app, "init", "-q")
        (app / "third-party.lock.toml").write_text(
            '[[dependency]]\nname = "PipeFixture"\nrevision = "' + revision + '"\n', encoding="utf-8")
        commit(app)
        cached = app / "target/native-sources" / ("pipefixture-" + revision)
        cached.parent.mkdir(parents=True)
        shutil.move(str(upstream), cached)
        target = base / "sources.tar.gz"
        try:
            ROOT = app
            sources(target, "fixture", False)
        finally:
            ROOT = original
        with tarfile.open(target) as archive:
            with archive.extractfile("MetaXtract-sources/upstream/PipeFixture/payload.bin") as stream:
                if stream.read() != payload:
                    raise ValueError("source archive pipe changed the payload")
    print(json.dumps({"source_archive_pipe": "passed", "fixture_bytes": 8192}))


def checksum_file(path: Path) -> None:
    path.with_name(path.name + ".sha256").write_text(sha256(path) + "  " + path.name + "\n", encoding="ascii")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=ROOT / "dist")
    parser.add_argument("--allow-dirty", action="store_true", help="mark an uncommitted local verification build")
    parser.add_argument("--check-source-archives", action="store_true", help="check source archive pipes without compiling")
    parser.add_argument("--source-archive-fixture", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.source_archive_fixture:
        source_archive_fixture()
        return 0
    if args.check_source_archives:
        subprocess.run([sys.executable, str(Path(__file__).resolve()), "--source-archive-fixture"],
                       check=True, timeout=60)
        return 0
    os.chdir(ROOT)
    dirty = bool(runtime.output("git", "status", "--porcelain", "--untracked-files=normal"))
    if dirty and not args.allow_dirty:
        raise ValueError("release source is modified; commit it or use --allow-dirty for local verification")
    revision = runtime.output("git", "rev-parse", "HEAD").strip()
    target = next(line.split(": ", 1)[1] for line in runtime.output("rustc", "-vV").splitlines() if line.startswith("host: "))
    programs, native = build()
    if runtime.output("git", "rev-parse", "HEAD").strip() != revision or (
        not args.allow_dirty and runtime.output("git", "status", "--porcelain", "--untracked-files=normal")
    ):
        raise ValueError("release source changed during compilation")
    args.out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="metaxtract-package-", dir=args.out) as temporary:
        stage = Path(temporary)
        root = stage / "MetaXtract"
        root.mkdir()
        manifest = assemble(root, programs, native)
        source = stage / ("MetaXtract-sources-" + target + ".tar.gz")
        included = sources(source, revision, dirty)
        manifest.update({"schema": 1, "target": target, "git_commit": revision, "source_dirty": dirty,
                         "native_sources": included, "source_archive": source.name,
                         "source_sha256": sha256(source), "rustc": runtime.output("rustc", "--version").strip(),
                         "directories": [p.relative_to(root).as_posix() for p in sorted(root.rglob("*")) if p.is_dir()],
                         "files": {p.relative_to(root).as_posix(): {"sha256": sha256(p), "bytes": p.stat().st_size}
                                   for p in sorted(root.rglob("*")) if p.is_file()}})
        (root / "package.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
        name = "MetaXtract-" + target
        if os.name == "nt":
            archive = stage / (name + ".zip")
            with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as stream:
                for path in sorted(root.rglob("*")):
                    stream.write(path, path.relative_to(stage).as_posix())
        else:
            archive = stage / (name + ".tar.gz")
            with tarfile.open(archive, "w:gz") as stream:
                stream.add(root, "MetaXtract")
        for path in (source, archive):
            destination = args.out / path.name
            os.replace(path, destination)
            checksum_file(destination)
        print(json.dumps({"package": str(args.out / archive.name), "sources": str(args.out / source.name),
                          "target": target, "git_commit": revision, "source_dirty": dirty}))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"package build failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
