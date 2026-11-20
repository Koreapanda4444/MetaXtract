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
import time
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

    def payloads(entry: int, depth: int = 0):
        if depth > 4:
            raise ValueError("PE resource nesting exceeds limit")
        if not entry & 0x80000000:
            address, length = struct.unpack_from("<II", data, base + entry)
            at = offset(address)
            yield data[at : at + length]
            return
        directory = base + (entry & 0x7FFFFFFF)
        named, ids = struct.unpack_from("<HH", data, directory + 12)
        if named + ids > 4096:
            raise ValueError("PE resource count exceeds limit")
        for n in range(named + ids):
            child = struct.unpack_from("<II", data, directory + 16 + n * 8)[1]
            yield from payloads(child, depth + 1)

    for kind in (3, 14, 16):
        if kind not in entries or not next(payloads(entries[kind]), b""):
            raise ValueError("EXE lacks embedded icon/group/version resources")
    version = next(payloads(entries[16]))
    for text in (
        "ProductName",
        "MetaXtract",
        "FileDescription",
        "FileVersion",
        "ProductVersion",
    ):
        if text.encode("utf-16le") not in version:
            raise ValueError("EXE version resource is incomplete: " + text)

    def valid_icon(icon: bytes) -> bool:
        if icon.startswith(b"\x89PNG\r\n\x1a\n"):
            return len(icon) >= 24
        if len(icon) < 40:
            return False
        header, width, height, planes, bits = struct.unpack_from("<IiiHH", icon)
        return (
            header >= 40
            and width > 0
            and height > 0
            and height % 2 == 0
            and planes == 1
            and bits in (1, 4, 8, 24, 32)
        )

    if not any(valid_icon(icon) for icon in payloads(entries[3])):
        raise ValueError("EXE contains no valid icon image")


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

        def run(*args, graphics_driver=False, nsis_directory=None):
            args = list(args)
            if not Path(str(args[0])).is_absolute():
                args[0] = shutil.which(str(args[0])) or str(args[0])
            command = list(map(str, args))
            if nsis_directory is not None:
                if os.name != "nt":
                    raise ValueError("NSIS directory options require Windows")
                # NSIS requires /D= to be the final, unquoted command-line tail.
                command = (
                    subprocess.list2cmdline(command) + " /D=" + str(nsis_directory)
                )
            with windows_graphics(graphics if graphics_driver else None):
                result = subprocess.run(
                    command,
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
            pe_resources(setup)
            import winreg

            try:
                with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\MetaXtract"):
                    raise ValueError(
                        "Native installer checks require a clean disposable Windows profile"
                    )
            except FileNotFoundError:
                pass
            app = base / "Installed application"
            run(setup, "/S", nsis_directory=app)
            manifest = runtime(app)
            for role in ("launcher", "gui"):
                pe_resources(app / manifest["executables"][role])
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
            run(setup, "/S", nsis_directory=app)
            runtime(app)
            if retained.read_text() != "preserve evidence":
                raise ValueError("Upgrade erased user evidence")
            run(app / "Uninstall.exe", "/S")
            deadline = time.monotonic() + 60
            while (app / "Uninstall.exe").exists() and time.monotonic() < deadline:
                time.sleep(0.1)
            if (
                any((app / name).exists() for name in manifest["files"])
                or (app / "Uninstall.exe").exists()
            ):
                raise ValueError("Uninstaller retained an owned application file")
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
            if Path("/opt/MetaXtract").exists():
                raise ValueError(
                    "Native package checks require a clean disposable Linux installation"
                )
            # Runner is disposable; dpkg owns only files declared by the package.
            subprocess.run(
                [
                    "sudo",
                    "apt-get",
                    "install",
                    "--reinstall",
                    "--no-install-recommends",
                    "-y",
                    str(deb),
                ],
                check=True,
                timeout=180,
            )
            app = Path("/opt/MetaXtract")
            runtime(app)
            retained = app / "verification-user-evidence.txt"
            subprocess.run(["sudo", "cp", str(evidence), str(retained)], check=True)
            subprocess.run(
                [
                    "sudo",
                    "apt-get",
                    "install",
                    "--reinstall",
                    "--no-install-recommends",
                    "-y",
                    str(deb),
                ],
                check=True,
                timeout=180,
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
            subprocess.run(["sudo", "rmdir", "--", str(app)], check=True)
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
