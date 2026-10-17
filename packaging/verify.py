"""Verify portable installation, upgrade/rollback and removal in a private workspace."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import sqlite3
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import zipfile
import zlib
from contextlib import closing
from pathlib import Path, PurePosixPath


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def relative(name: str) -> str:
    if not name or "\\" in name or ":" in name or any(p in ("", ".", "..") for p in name.split("/")):
        raise ValueError("unsafe package path")
    path = PurePosixPath(name)
    if path.is_absolute() or path.parts[0] != "MetaXtract":
        raise ValueError("unexpected package root")
    return name


def check_inventory(root: Path) -> dict:
    if root.is_symlink() or not root.is_dir():
        raise ValueError("application root must be a real directory")
    manifest = json.loads((root / "package.json").read_text(encoding="utf-8"))
    if manifest.get("schema") != 1 or not isinstance(manifest.get("files"), dict):
        raise ValueError("unsupported package inventory")
    actual = {p.relative_to(root).as_posix() for p in root.rglob("*") if p.is_file()}
    if actual != set(manifest["files"]) | {"package.json"}:
        raise ValueError("package contains missing or unowned files")
    directories = {p.relative_to(root).as_posix() for p in root.rglob("*") if p.is_dir()}
    if directories != set(manifest["directories"]) or any(p.is_symlink() for p in root.rglob("*")):
        raise ValueError("package contains links or unowned directories")
    for name in directories:
        relative("MetaXtract/" + name)
    for name, expected in manifest["files"].items():
        relative("MetaXtract/" + name)
        path = root / name
        if path.is_symlink() or path.stat().st_size != expected["bytes"] or digest(path) != expected["sha256"]:
            raise ValueError("package file checksum differs: " + name)
        if path.suffix.lower() in (".py", ".pyc") or path.name.lower().removesuffix(".exe") in ("python", "python3", "cargo", "rustc", "exiftool", "ffprobe"):
            raise ValueError("build tool or metadata executable in runtime package: " + name)
    for name in manifest["executables"].values():
        if name not in manifest["files"]:
            raise ValueError("executable is absent from inventory")
    return manifest


def check_sources(archive: Path, manifest: dict) -> None:
    name = manifest["source_archive"]
    if Path(name).name != name:
        raise ValueError("unsafe source archive path")
    source = archive.parent / name
    expected = source.with_name(source.name + ".sha256").read_text(encoding="ascii").split()
    actual = digest(source)
    if expected != [actual, name] or actual != manifest["source_sha256"]:
        raise ValueError("matching source archive checksum differs")
    with tarfile.open(source) as stream:
        with stream.extractfile("MetaXtract-sources/provenance.json") as provenance:
            value = json.load(provenance)
    if value["git_commit"] != manifest["git_commit"] or value["native_sources"] != manifest["native_sources"]:
        raise ValueError("source archive provenance differs")


def unpack(archive: Path, directory: Path) -> Path:
    expected = archive.with_name(archive.name + ".sha256").read_text(encoding="ascii").split()
    if expected != [digest(archive), archive.name]:
        raise ValueError("archive checksum differs")
    seen, total = set(), 0

    def write(name: str, size: int, mode: int, stream) -> None:
        nonlocal total
        name = relative(name)
        if name.casefold() in seen or len(seen) >= 20000 or size < 0 or size > 1024 ** 3:
            raise ValueError("duplicate or oversized package entry")
        seen.add(name.casefold())
        total += size
        if total > 2 * 1024 ** 3:
            raise ValueError("package expansion exceeds 2 GiB")
        path = directory / name
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("xb") as target:
            remaining = size
            while remaining:
                block = stream.read(min(remaining, 1024 * 1024))
                if not block:
                    raise ValueError("truncated package entry")
                target.write(block)
                remaining -= len(block)
        if os.name != "nt":
            path.chmod(mode & 0o777)

    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as source:
            for item in source.infolist():
                if item.is_dir():
                    relative(item.filename.rstrip("/"))
                    (directory / item.filename).mkdir(parents=True, exist_ok=True)
                    continue
                mode = item.external_attr >> 16
                if stat.S_IFMT(mode) not in (0, stat.S_IFREG):
                    raise ValueError("special package entry")
                with source.open(item) as stream:
                    write(item.filename, item.file_size, mode or 0o644, stream)
    else:
        with tarfile.open(archive) as source:
            for item in source:
                if item.isdir():
                    relative(item.name)
                    (directory / item.name).mkdir(parents=True, exist_ok=True)
                    continue
                if not item.isfile():
                    raise ValueError("package links and special entries are forbidden")
                with source.extractfile(item) as stream:
                    write(item.name, item.size, item.mode, stream)
    root = directory / "MetaXtract"
    check_inventory(root)
    return root


def remove(root: Path) -> None:
    check_inventory(root)  # Refuse to erase cases or unrelated files left in the app folder.
    shutil.rmtree(root)


def activate(archive: Path, destination: Path, probe) -> None:
    """Stage before touching the installed tree; restore it if activation fails."""
    with tempfile.TemporaryDirectory(prefix="package-stage-", dir=destination.parent) as temporary:
        prepared = unpack(archive, Path(temporary))
        backup = Path(temporary) / "previous"
        if destination.exists():
            check_inventory(destination)
            os.replace(destination, backup)
        try:
            os.replace(prepared, destination)
            probe(destination)
        except BaseException:
            if destination.exists():
                remove(destination)
            if backup.exists():
                os.replace(backup, destination)
            raise
        if backup.exists():
            remove(backup)


def environment(root: Path) -> dict[str, str]:
    keep = {"SystemRoot", "SYSTEMROOT", "WINDIR", "windir", "SystemDrive", "COMSPEC",
            "LANG", "LC_ALL", "LC_CTYPE", "DISPLAY", "XAUTHORITY", "LIBGL_ALWAYS_SOFTWARE"}
    env = {k: v for k, v in os.environ.items() if k in keep}
    for name in ("home", "config", "temporary", "empty-path", "runtime"):
        (root / name).mkdir(exist_ok=True, mode=0o700)
    env.update({"PATH": str(root / "empty-path"), "HOME": str(root / "home"),
                "USERPROFILE": str(root / "home"), "APPDATA": str(root / "config"),
                "LOCALAPPDATA": str(root / "config"), "XDG_CONFIG_HOME": str(root / "config"),
                "XDG_RUNTIME_DIR": str(root / "runtime"), "TMPDIR": str(root / "temporary"),
                "TMP": str(root / "temporary"), "TEMP": str(root / "temporary")})
    return env


def invoke(app: Path, env: dict, *args: object, role: str = "cli", success: bool = True,
           json_output: bool = True):
    manifest = check_inventory(app)
    executable = app / manifest["executables"][role]
    result = subprocess.run([str(executable), *map(str, args)], cwd=app.parent,
                            env=env, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60)
    if success != (result.returncode == 0):
        raise ValueError(f"{role} {args[0]} exit {result.returncode}: {result.stderr[-2000:]} {result.stdout[:1000]}")
    if not success or not json_output:
        return result
    envelope = json.loads(result.stdout)
    if not envelope["ok"]:
        raise ValueError("CLI reported failure")
    return envelope["result"]


def generated_schema_five(source: Path, destination: Path) -> None:
    """Project our generated case onto the pre-audit schema-5 table set."""
    shutil.copyfile(source, destination)
    retained = {"case_info", "scans", "records", "raw_fields", "normalized_claims", "conflicts",
                "diagnostics", "case_notes", "annotations", "saved_filters", "imports",
                "import_artifacts", "legacy_records", "scan_receipts"}
    with closing(sqlite3.connect(destination)) as db, db:
        for (name,) in db.execute("SELECT name FROM sqlite_master WHERE type='trigger'").fetchall():
            db.execute('DROP TRIGGER "' + name.replace('"', '""') + '"')
        for (name,) in db.execute("SELECT name FROM sqlite_master WHERE type='table'").fetchall():
            if name not in retained and not name.startswith("sqlite_"):
                db.execute('DROP TABLE "' + name.replace('"', '""') + '"')
        db.execute("PRAGMA user_version=5")


def workflow(current: Path, previous: Path | None, gui: bool) -> dict:
    with tempfile.TemporaryDirectory(prefix="MetaXtract install 근거 ") as temporary:
        root = Path(temporary)
        app = root / "Application with spaces 한글"
        env = environment(root)
        evidence = root / "evidence 한글.png"

        def chunk(kind: bytes, data: bytes) -> bytes:
            return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff)

        evidence.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0))
                             + chunk(b"tEXt", b"Software\0MetaXtract isolated package")
                             + chunk(b"IDAT", zlib.compress(b"\0\xff\0\0")) + chunk(b"IEND", b""))
        initial = previous or current
        activate(initial, app, lambda path: invoke(path, env, "--help", json_output=False))
        old = check_inventory(app)
        check_sources(initial, old)
        invoke(app, env, "--help", role="gui", json_output=False)
        case = root / "investigation.mxcase"
        scan = invoke(app, env, "scan", evidence, "--case", case, "--workers", 1)
        if scan["summary"]["failed"] or scan["summary"]["completed"] != 1:
            raise ValueError("installed worker failed to extract the evidence")
        record = invoke(app, env, "search", case, scan["scan_id"])["items"][0]["id"]
        preview = invoke(app, env, "preview", case, record, evidence)
        if not preview["verified"] or preview["rgb"] != [255, 0, 0]:
            raise ValueError("bundled native decoder did not return the known pixels")
        records = root / "before.jsonl"
        invoke(app, env, "export", case, scan["scan_id"], records)
        if "MetaXtract isolated package" not in records.read_text(encoding="utf-8"):
            raise ValueError("deep PNG metadata is absent")
        invoke(app, env, "case", "notes", case, "Preserved note / 보존할 메모")
        identity = invoke(app, env, "case", "info", case)
        saved = digest(case)
        settings = root / "config/retained-settings.txt"
        settings.write_text("preserve my settings / 설정 유지", encoding="utf-8")
        settings_digest = digest(settings)

        # A failed activation must restore the whole previous application tree.
        def rejected(_):
            raise RuntimeError("simulated activation failure")
        try:
            activate(current, app, rejected)
        except RuntimeError:
            pass
        else:
            raise ValueError("failed activation unexpectedly succeeded")
        if check_inventory(app)["git_commit"] != old["git_commit"] or digest(case) != saved:
            raise ValueError("upgrade rollback lost the installed application or case")

        # Unknown files must prevent replacement and automatic removal.
        extra = app / "user-evidence.txt"
        extra.write_text("keep", encoding="ascii")
        for action in (lambda: activate(current, app, lambda _: None), lambda: remove(app)):
            try:
                action()
            except ValueError:
                pass
            else:
                raise ValueError("an unowned file was silently removed")
        if extra.read_text(encoding="ascii") != "keep":
            raise ValueError("unowned evidence changed")
        extra.unlink()

        def reopened(path):
            if check_inventory(path)["target"] != old["target"]:
                raise ValueError("package architecture differs from the installed application")
            if invoke(path, env, "case", "info", case) != identity:
                raise ValueError("case identity or notes changed during upgrade")
            if invoke(path, env, "verify", case)["records"] != 1:
                raise ValueError("upgraded application lost case records")
        activate(current, app, reopened)
        new = check_inventory(app)
        check_sources(current, new)
        after = root / "after.jsonl"
        invoke(app, env, "export", case, scan["scan_id"], after)
        if digest(after) != digest(records) or digest(case) != saved:
            raise ValueError("upgrade changed persisted metadata")

        # Writable migration keeps pre-audit payloads; read-only opens must not migrate.
        legacy = root / "schema-five.mxcase"
        generated_schema_five(case, legacy)
        original = digest(legacy)
        invoke(app, env, "verify", legacy, success=False)
        if digest(legacy) != original:
            raise ValueError("read-only verification changed an old case")
        invoke(app, env, "case", "notes", legacy, "Migrated note / 이관한 메모")
        if invoke(app, env, "verify", legacy)["records"] != 1:
            raise ValueError("schema migration lost metadata")
        migrated = root / "migrated.jsonl"
        invoke(app, env, "export", legacy, scan["scan_id"], migrated)
        if digest(migrated) != digest(records):
            raise ValueError("schema migration rewrote raw records")

        # A future schema must be refused without writing to the evidence case.
        future = root / "future.mxcase"
        shutil.copyfile(case, future)
        with closing(sqlite3.connect(future)) as db, db:
            db.execute("PRAGMA user_version=2147483647")
        original = digest(future)
        invoke(app, env, "case", "notes", future, "must not save", success=False)
        if digest(future) != original:
            raise ValueError("future case was modified")
        if gui:
            invoke(app, env, case, "--smoke", role="gui", json_output=False)
        desktop = (root / "home/Library/Application Support" if sys.platform == "darwin"
                   else root / "config") / "MetaXtract/desktop.json"
        if gui and not desktop.is_file():
            raise ValueError("desktop did not preserve its recent-case settings")
        desktop_digest = digest(desktop) if gui else None

        # No global worker fallback is allowed when the bundled worker is missing.
        worker = app / new["executables"]["worker"]
        hidden = worker.with_name(worker.name + ".held")
        worker.rename(hidden)
        try:
            # Deliberately bypass the inventory guard to exercise the actual product error.
            result = subprocess.run([str(app / new["executables"]["cli"]), "inspect", str(evidence)],
                                    cwd=root, env=env, capture_output=True, timeout=60)
            if result.returncode == 0:
                raise ValueError("product silently replaced a missing bundled worker")
        finally:
            hidden.rename(worker)
        remove(app)
        if app.exists() or digest(case) != saved or digest(settings) != settings_digest or not evidence.is_file():
            raise ValueError("removal changed external evidence, cases or settings")
        if gui and digest(desktop) != desktop_digest:
            raise ValueError("removal changed desktop settings")
        return {"ok": True, "target": new["target"], "git_commit": new["git_commit"],
                "previous_commit": old["git_commit"], "upgrade_kind": "previous-package" if previous else "reinstall",
                "installation": True, "upgrade": True, "rollback": True, "schema_five_migration": True,
                "future_schema_refused": True, "unowned_files_retained": True, "removal": True,
                "desktop_smoke": gui, "preserved_records": 1, "decoder_rgb": preview["rgb"]}


def find_package(directory: Path) -> Path:
    values = [p for p in directory.glob("MetaXtract-*") if p.is_file()
              and p.name.endswith((".zip", ".tar.gz")) and not p.name.startswith("MetaXtract-sources-")]
    if len(values) != 1:
        raise ValueError("expected one native-host application archive")
    return values[0].resolve()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dist", type=Path, default=Path("dist"))
    parser.add_argument("--previous", type=Path, help="directory containing a previous successful native-host package")
    parser.add_argument("--previous-if-present", type=Path, help="CI baseline directory, if a previous artifact is available")
    parser.add_argument("--gui-smoke", action="store_true")
    args = parser.parse_args()
    current = find_package(args.dist)
    if args.previous and args.previous_if_present:
        raise ValueError("choose one previous-package option")
    baseline = args.previous or (args.previous_if_present if args.previous_if_present and args.previous_if_present.exists() else None)
    previous = find_package(baseline) if baseline else None
    result = workflow(current, previous, args.gui_smoke)
    destination = args.dist / "installation-verification.json"
    destination.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, subprocess.SubprocessError, zipfile.BadZipFile, tarfile.TarError) as error:
        print(f"installation verification failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
