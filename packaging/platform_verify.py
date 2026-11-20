"""Exercise real native distribution formats in isolated locations."""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path


def pe_resources(path: Path) -> None:
    data = path.read_bytes()
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[:2] != b"MZ" or data[pe : pe + 4] != b"PE\0\0":
        raise ValueError("Invalid Windows executable")
    count, size = struct.unpack_from("<H12xH", data, pe + 6)
    optional = pe + 24
    magic = struct.unpack_from("<H", data, optional)[0]
    directories = optional + (112 if magic == 0x20B else 96)
    rva = struct.unpack_from("<I", data, directories + 16)[0]

    def offset(address: int) -> int:
        for n in range(count):
            virtual, start, raw_size, raw = struct.unpack_from(
                "<IIII", data, optional + size + n * 40 + 8
            )
            if start <= address < start + max(virtual, raw_size):
                return raw + address - start
        raise ValueError("Invalid PE resource address")

    base = offset(rva)
    named, ids = struct.unpack_from("<HH", data, base + 12)
    entries = {
        struct.unpack_from("<II", data, base + 16 + n * 8)[0]: struct.unpack_from(
            "<II", data, base + 16 + n * 8
        )[1]
        for n in range(named + ids)
    }

    def payload(entry: int) -> bytes:
        for _ in range(4):
            if not entry & 0x80000000:
                address, length = struct.unpack_from("<II", data, base + entry)
                at = offset(address)
                return data[at : at + length]
            directory = base + (entry & 0x7FFFFFFF)
            entry = struct.unpack_from("<II", data, directory + 16)[1]
        raise ValueError("PE resource nesting exceeds limit")

    for kind in (3, 14, 16):
        if kind not in entries or not payload(entries[kind]):
            raise ValueError("EXE lacks embedded icon/group/version resources")
    version = payload(entries[16])
    for text in (
        "ProductName",
        "MetaXtract",
        "FileDescription",
        "FileVersion",
        "ProductVersion",
    ):
        if text.encode("utf-16le") not in version:
            raise ValueError("EXE version resource is incomplete: " + text)
    if b"\x89PNG\r\n\x1a\n" not in payload(entries[3]):
        raise ValueError("EXE icon does not contain the application artwork")


def verify_formats(directory: Path, gui: bool, graphics: Path | None) -> dict:
    from verify import environment, windows_graphics

    assets = directory.resolve()
    with tempfile.TemporaryDirectory(
        prefix="MetaXtract native install 한글 "
    ) as temporary:
        base = Path(temporary)
        env = environment(base)
        evidence = (
            Path(__file__).resolve().parents[1]
            / "crates/metaxtract-gui/assets/icon.png"
        )

        def run(*args, graphics_driver=False):
            args = list(args)
            if not Path(str(args[0])).is_absolute():
                args[0] = shutil.which(str(args[0])) or str(args[0])
            with windows_graphics(graphics if graphics_driver else None):
                result = subprocess.run(
                    list(map(str, args)),
                    env=env,
                    cwd=base,
                    capture_output=True,
                    timeout=120,
                )
            if result.returncode:
                raise ValueError(
                    f"Native distribution failed: {args[0]}: {result.stderr[-2000:]!r}"
                )
            return result

        def runtime(app: Path):
            manifest = json.loads((app / "package.json").read_text(encoding="utf-8"))
            for name, expected in manifest["files"].items():
                with (app / name).open("rb") as stream:
                    if (
                        hashlib.file_digest(stream, "sha256").hexdigest()
                        != expected["sha256"]
                    ):
                        raise ValueError("Installed runtime differs: " + name)
            run(app / manifest["executables"]["cli"], "inspect", evidence)
            if gui:
                executable = app / manifest["executables"].get(
                    "launcher", manifest["executables"]["gui"]
                )
                run(executable, "--smoke", graphics_driver=True)
            return manifest

        if os.name == "nt":
            (setup,) = assets.glob("*-Windows-Setup.exe")
            app = base / "Installed application"
            run(setup, "/S", f"/D={app}")
            manifest = runtime(app)
            for role in ("launcher", "gui"):
                pe_resources(app / manifest["executables"][role])
            import winreg

            key = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\MetaXtract"
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, key) as registry:
                if (
                    str(app / "MetaXtract.exe")
                    not in winreg.QueryValueEx(registry, "DisplayIcon")[0]
                ):
                    raise ValueError(
                        "Uninstall icon does not reference the installed EXE"
                    )
            retained = app / "user-evidence.txt"
            retained.write_text("preserve evidence", encoding="utf-8")
            run(setup, "/S", f"/D={app}")
            runtime(app)
            if retained.read_text() != "preserve evidence":
                raise ValueError("Upgrade erased user evidence")
            run(app / "Uninstall.exe", "/S", f"_?={app}")
            if (
                retained.read_text() != "preserve evidence"
                or (app / "MetaXtract.exe").exists()
            ):
                raise ValueError("Uninstaller ownership verification failed")
            try:
                winreg.OpenKey(winreg.HKEY_CURRENT_USER, key)
            except FileNotFoundError:
                pass
            else:
                raise ValueError("Uninstall registration was retained")
            return {
                "nsis_install_upgrade_remove": True,
                "embedded_exe_resources": True,
                "unowned_data_preserved": True,
            }
        if sys.platform == "darwin":
            (image,) = assets.glob("*.dmg")
            mount = base / "Mounted image"
            mount.mkdir()
            run(
                "hdiutil",
                "attach",
                image,
                "-readonly",
                "-nobrowse",
                "-mountpoint",
                mount,
            )
            try:
                app = base / "Applications/MetaXtract.app"
                app.parent.mkdir()
                shutil.copytree(mount / "MetaXtract.app", app)
                run("codesign", "--verify", "--deep", "--strict", app)
                run(app / "Contents/MacOS/metaxtract", "inspect", evidence)
                if gui:
                    run(app / "Contents/MacOS/metaxtract-gui", "--smoke")
            finally:
                run("hdiutil", "detach", mount)
            shutil.rmtree(app)
            return {"dmg_mount_copy_run_remove": True, "notarization_claimed": False}
        (image,) = assets.glob("*.AppImage")
        image.chmod(0o755)
        run(image, "--appimage-extract-and-run", "--cli", "inspect", evidence)
        if gui:
            run(image, "--appimage-extract-and-run", "--smoke")
        (deb,) = assets.glob("*.deb")
        if shutil.which("sudo"):
            # Runner is disposable; dpkg owns only files declared by the package.
            subprocess.run(
                ["sudo", "dpkg", "--install", str(deb)], check=True, timeout=120
            )
            app = Path("/opt/MetaXtract")
            runtime(app)
            retained = app / "verification-user-evidence.txt"
            subprocess.run(["sudo", "cp", str(evidence), str(retained)], check=True)
            subprocess.run(
                ["sudo", "dpkg", "--install", str(deb)], check=True, timeout=120
            )
            runtime(app)
            subprocess.run(
                ["sudo", "dpkg", "--remove", "metaxtract"], check=True, timeout=120
            )
            if (
                retained.read_bytes() != evidence.read_bytes()
                or (app / "MetaXtract").exists()
            ):
                raise ValueError("DEB removal changed unowned evidence")
            subprocess.run(["sudo", "rm", "--", str(retained)], check=True)
            native = True
        else:
            run("dpkg-deb", "--extract", deb, base / "deb-extracted")
            runtime(base / "deb-extracted/opt/MetaXtract")
            native = False
        return {
            "appimage_extraction_run": True,
            "deb_install_upgrade_remove": native,
            "deb_extracted_run": not native,
        }
