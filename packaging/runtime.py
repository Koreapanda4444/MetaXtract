"""Build-time dependency inspection; never included in the runtime package."""
from __future__ import annotations

import os
import re
import struct
import subprocess
import sys
from pathlib import Path

LINUX_SYSTEM = {
    "libc.so.6", "libm.so.6", "libdl.so.2", "libpthread.so.0", "librt.so.1",
    "libgcc_s.so.1", "libstdc++.so.6", "libutil.so.1", "libresolv.so.2",
    "libX11.so.6", "libXcursor.so.1", "libXi.so.6", "libXrandr.so.2",
    "libXinerama.so.1", "libxcb.so.1", "libxcb-xkb.so.1", "libxkbcommon.so.0",
    "libxkbcommon-x11.so.0", "libGL.so.1", "libEGL.so.1", "libudev.so.1",
    "libwayland-client.so.0", "libwayland-cursor.so.0", "libwayland-egl.so.1",
}
WINDOWS_SYSTEM = set((
    "kernel32 kernelbase ntdll advapi32 bcrypt bcryptprimitives userenv shell32 ws2_32 "
    "crypt32 user32 gdi32 ole32 oleaut32 comdlg32 shlwapi imm32 winmm comctl32 "
    "winspool usp10 dwmapi dwrite d3d11 d3d12 dxgi opengl32 secur32 setupapi cfgmgr32 "
    "propsys version apphelp powrprof hid wtsapi32 runtimeobject ncrypt netapi32 "
    "msimg32 mswsock uxtheme wintrust wldap32 dnsapi msasn1 winhttp iphlpapi shcore "
    "dbghelp ucrtbase avrt dcomp win32u normaliz psapi profapi windowscodecs "
    "combase dxcore d2d1 d3dcompiler_47 msctf wininet"
).split())


def output(*args: str | Path) -> str:
    return subprocess.check_output(list(map(str, args)), text=True, encoding="utf-8")


def pe_imports(path: Path) -> list[str]:
    data = path.read_bytes()
    if data[:2] != b"MZ":
        raise ValueError(f"not a PE executable: {path}")
    pe = struct.unpack_from("<I", data, 0x3c)[0]
    if data[pe:pe + 4] != b"PE\0\0":
        raise ValueError(f"invalid PE signature: {path}")
    sections, optional_size = struct.unpack_from("<H12xH", data, pe + 6)
    optional = pe + 24
    magic = struct.unpack_from("<H", data, optional)[0]
    if magic not in (0x10b, 0x20b):
        raise ValueError("unknown PE optional header")
    directories = optional + (112 if magic == 0x20b else 96)
    image_base = struct.unpack_from("<Q" if magic == 0x20b else "<I", data,
                                    optional + (24 if magic == 0x20b else 28))[0]

    def offset(rva: int) -> int:
        for i in range(sections):
            virtual_size, start, raw_size, raw = struct.unpack_from(
                "<IIII", data, optional + optional_size + i * 40 + 8)
            if start <= rva < start + max(virtual_size, raw_size):
                result = raw + rva - start
                if result >= len(data) or rva - start >= raw_size:
                    break
                return result
        raise ValueError(f"invalid PE dependency address: {path}")

    names = set()
    for index, width, name_offset in [(1, 20, 12), (13, 32, 4)]:
        if directories + (index + 1) * 8 > optional + optional_size:
            continue
        rva, size = struct.unpack_from("<II", data, directories + index * 8)
        if not rva or not size:
            continue
        start = offset(rva)
        for i in range(min(size // width + 1, 4096)):
            entry = start + i * width
            block = data[entry:entry + width]
            if len(block) != width:
                raise ValueError("truncated PE import table")
            if not any(block):
                break
            name = struct.unpack_from("<I", block, name_offset)[0]
            if index == 13 and not struct.unpack_from("<I", block)[0] & 1:
                name -= image_base
            at = offset(name)
            end = data.find(b"\0", at, at + 1024)
            if end < 0:
                raise ValueError("unterminated PE dependency name")
            names.add(data[at:end].decode("ascii").lower())
        else:
            raise ValueError("unbounded PE import table")
    return sorted(names)


def imports(path: Path) -> list[str]:
    if os.name == "nt":
        return pe_imports(path)
    if sys.platform == "darwin":
        return [line.strip().split(" (compatibility version", 1)[0]
                for line in output("otool", "-L", path).splitlines()[1:]]
    return re.findall(r"\(NEEDED\).*?\[(.*?)\]", output("readelf", "-d", path))


def library_name(path: Path) -> str:
    if os.name == "nt":
        return path.name
    if sys.platform == "darwin":
        return Path(output("otool", "-D", path).splitlines()[1].strip()).name
    names = re.findall(r"\(SONAME\).*?\[(.*?)\]", output("readelf", "-d", path))
    if len(names) != 1:
        raise ValueError(f"shared library is missing its SONAME: {path}")
    return names[0]


def system_library(name: str) -> bool:
    if os.name == "nt":
        lowered = name.lower()
        return lowered.startswith(("api-ms-win-", "ext-ms-win-")) or lowered.removesuffix(".dll") in WINDOWS_SYSTEM
    if sys.platform == "darwin":
        return name.startswith(("/usr/lib/", "/System/Library/"))
    return name in LINUX_SYSTEM or name.startswith(("ld-linux-", "ld64.so"))


def windows_crt() -> Path:
    """Use the licensed toolchain's REDIST tree, never a system32 DLL."""
    roots = []
    for name in ("VCToolsRedistDir", "VCINSTALLDIR", "VSINSTALLDIR"):
        if os.environ.get(name):
            roots.append(Path(os.environ[name]))
    where = Path(os.environ.get("ProgramFiles(x86)", "C:/Program Files (x86)")) / "Microsoft Visual Studio/Installer/vswhere.exe"
    if where.is_file():
        installation = output(where, "-latest", "-products", "*", "-requires",
                              "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath").strip()
        if installation:
            roots.append(Path(installation))
    candidates = []
    for root in roots:
        for pattern in ("x64/Microsoft.VC*.CRT", "Redist/MSVC/*/x64/Microsoft.VC*.CRT",
                        "VC/Redist/MSVC/*/x64/Microsoft.VC*.CRT"):
            candidates.extend(root.glob(pattern))
    candidates = [p for p in candidates if (p / "vcruntime140.dll").is_file() and (p / "msvcp140.dll").is_file()]
    if not candidates:
        raise ValueError("MSVC x64 app-local REDIST runtime not found; install the C++ build tools")
    return max(candidates, key=lambda p: tuple(int(n) for n in re.findall(r"\d+", str(p))))


def inspect(root: Path, files: list[Path]) -> dict:
    """Reject undeclared non-OS dependencies and absolute build-directory links."""
    bundled = {p.name.lower(): p for p in files}
    dependencies, system = {}, set()
    versions = {name: set() for name in ("GLIBC", "GLIBCXX", "CXXABI")}
    macos = set()
    for path in files:
        names = imports(path)
        dependencies[path.relative_to(root).as_posix()] = names
        for name in names:
            if system_library(name):
                system.add(name)
                continue
            if Path(name).name.lower() not in bundled:
                raise ValueError(f"unbundled dependency: {path.name} -> {name}")
            if sys.platform.startswith("linux") and name != Path(name).name:
                raise ValueError(f"non-local native dependency: {name}")
            if sys.platform == "darwin" and not name.startswith(("@rpath/", "@loader_path/", "@executable_path/")):
                raise ValueError(f"absolute non-OS dependency: {name}")
        if sys.platform.startswith("linux"):
            dynamic = output("readelf", "-d", path)
            for value in re.findall(r"\((?:RUNPATH|RPATH)\).*?\[(.*?)\]", dynamic):
                if any(part and not part.startswith("$ORIGIN") for part in value.split(":")):
                    raise ValueError(f"non-relative runtime path in {path}: {value}")
            version_info = output("readelf", "--version-info", path)
            for name, values in versions.items():
                values.update(re.findall(r"\b" + name + r"_(\d+(?:\.\d+)+)", version_info))
        if sys.platform == "darwin":
            for command in output("otool", "-l", path).split("Load command"):
                if "cmd LC_BUILD_VERSION" in command:
                    macos.update(re.findall(r"\bminos (\d+(?:\.\d+)+)", command))
                elif "cmd LC_VERSION_MIN_MACOSX" in command:
                    macos.update(re.findall(r"\bversion (\d+(?:\.\d+)+)", command))
    def maximum(values: set[str]) -> str | None:
        return max(values, key=lambda s: tuple(map(int, s.split(".")))) if values else None
    return {"imports": dependencies, "system_libraries": sorted(system),
            **{name.lower() + "_minimum": maximum(values) for name, values in versions.items()},
            "macos_minimum": maximum(macos), "windows_minimum": "10" if os.name == "nt" else None}
