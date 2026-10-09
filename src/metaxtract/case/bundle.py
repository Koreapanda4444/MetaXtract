from __future__ import annotations

import hashlib
import stat
import tempfile
import zipfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from .paths import normalize_relative_path, resolve_source
from .redaction import sanitize_row
from .signing import SIGNATURE_MEMBER, sign_manifest
from ..core.files import PathLike, sha256_file
from ..core.jsonio import dumps_json, read_jsonl
from ..reporting.builder import build_report_from_rows


_ZIP_TIMESTAMP = (1980, 1, 1, 0, 0, 0)
_CONTROL_MEMBERS = (
    "manifest.json",
    "scan.jsonl",
    "hashes.txt",
    "reports/report.json",
)


def _sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _artifact(path: str, data: bytes) -> dict[str, Any]:
    return {
        "path": path,
        "sha256": _sha256_bytes(data),
        "size_bytes": len(data),
    }


def _created_at(records: list[dict[str, Any]]) -> str:
    timestamps = []
    for record in records:
        value = (record.get("metadata") or {}).get("mtime")
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            continue
        try:
            timestamps.append(datetime.fromtimestamp(value, timezone.utc))
        except (OverflowError, OSError, ValueError):
            continue
    if not timestamps:
        return "1980-01-01T00:00:00Z"
    return max(timestamps).isoformat().replace("+00:00", "Z")


def _zip_info(path: str, compression: int) -> zipfile.ZipInfo:
    info = zipfile.ZipInfo(path, date_time=_ZIP_TIMESTAMP)
    info.compress_type = compression
    info.create_system = 3
    info.external_attr = (stat.S_IFREG | 0o644) << 16
    info._compresslevel = 9
    return info


def _write_bytes(zf: zipfile.ZipFile, path: str, data: bytes) -> None:
    zf.writestr(
        _zip_info(path, zipfile.ZIP_DEFLATED),
        data,
        compress_type=zipfile.ZIP_DEFLATED,
        compresslevel=9,
    )


def _write_source(
    zf: zipfile.ZipFile,
    source: Path,
    member_path: str,
    expected_hash: str,
    expected_size: int,
) -> None:
    info = _zip_info(member_path, zipfile.ZIP_STORED)
    info.file_size = expected_size
    digest = hashlib.sha256()
    written = 0
    with source.open("rb") as input_stream:
        with zf.open(info, "w") as output_stream:
            while chunk := input_stream.read(1024 * 1024):
                output_stream.write(chunk)
                digest.update(chunk)
                written += len(chunk)
    if written != expected_size:
        raise ValueError(f"bundle source size changed: {member_path.removeprefix('files/')}")
    if digest.hexdigest() != expected_hash:
        raise ValueError(f"bundle source hash changed: {member_path.removeprefix('files/')}")


def export_case_bundle(
    scan_jsonl_path: PathLike,
    out_zip_path: PathLike,
    *,
    include_files: bool = False,
    redact: bool = False,
    case_id: str = None,
    notes: str = None,
    files_base: PathLike = None,
    signing_key: PathLike = None,
) -> None:
    scan_rows = read_jsonl(scan_jsonl_path)
    if redact and include_files:
        raise ValueError("--redact cannot be combined with --include-files")

    candidate_rows = [sanitize_row(row) for row in scan_rows] if redact else scan_rows
    output_rows = []
    seen_paths = set()
    for row in candidate_rows:
        normalized_path = normalize_relative_path(row.get("path"))
        collision_key = normalized_path.casefold()
        if collision_key in seen_paths:
            raise ValueError(f"duplicate bundle path: {normalized_path}")
        seen_paths.add(collision_key)
        normalized_row = dict(row)
        normalized_row["path"] = normalized_path
        output_rows.append(normalized_row)
    output_rows.sort(key=lambda row: row["path"])

    sources = []
    if include_files:
        base = Path(files_base) if files_base is not None else Path(scan_jsonl_path).parent
        if not base.exists():
            raise FileNotFoundError(f"files base does not exist: {base}")
        if not base.is_dir():
            raise NotADirectoryError(f"files base is not a directory: {base}")
        base = base.resolve(strict=True)
        for row in output_rows:
            source = resolve_source(base, row["path"])
            expected_hash = row["sha256"]
            expected_size = row["size_bytes"]
            if source.stat().st_size != expected_size:
                raise ValueError(f"bundle source size changed: {row['path']}")
            if sha256_file(source) != expected_hash:
                raise ValueError(f"bundle source hash changed: {row['path']}")
            sources.append((source, row["path"], expected_hash, expected_size))

    report = build_report_from_rows(output_rows)
    scan_data = ("\n".join(dumps_json(row) for row in output_rows) + "\n").encode("utf-8")
    hashes_data = (
        "\n".join(f"{row['sha256']}\t{row['path']}" for row in output_rows) + "\n"
    ).encode("utf-8")
    report_data = (dumps_json(report) + "\n").encode("utf-8")

    artifacts = [
        _artifact("scan.jsonl", scan_data),
        _artifact("hashes.txt", hashes_data),
        _artifact("reports/report.json", report_data),
    ]
    artifacts.extend(
        {
            "path": f"files/{relative_path}",
            "sha256": expected_hash,
            "size_bytes": expected_size,
        }
        for _source, relative_path, expected_hash, expected_size in sources
    )
    artifacts.sort(key=lambda item: item["path"])

    manifest = {
        "case_id": case_id,
        "created_at": _created_at(output_rows),
        "notes": notes or "",
        "record_count": len(output_rows),
        "hashes": [row["sha256"] for row in output_rows],
        "redacted": redact,
        "includes_files": include_files,
        "original_files": [
            {
                "path": relative_path,
                "sha256": expected_hash,
                "size_bytes": expected_size,
            }
            for _source, relative_path, expected_hash, expected_size in sources
        ],
        "artifacts": artifacts,
    }
    manifest_data = (dumps_json(manifest) + "\n").encode("utf-8")
    control_data = {
        "manifest.json": manifest_data,
        "scan.jsonl": scan_data,
        "hashes.txt": hashes_data,
        "reports/report.json": report_data,
    }
    control_members = list(_CONTROL_MEMBERS)
    if signing_key is not None:
        signature_data = (dumps_json(sign_manifest(manifest_data, signing_key)) + "\n").encode(
            "utf-8"
        )
        control_data[SIGNATURE_MEMBER] = signature_data
        control_members.insert(1, SIGNATURE_MEMBER)

    output_path = Path(out_zip_path)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    temp_handle = tempfile.NamedTemporaryFile(
        prefix=f".{output_path.name}.",
        suffix=".tmp",
        dir=output_path.parent,
        delete=False,
    )
    temp_path = Path(temp_handle.name)
    temp_handle.close()
    try:
        with zipfile.ZipFile(
            temp_path,
            "w",
            compression=zipfile.ZIP_DEFLATED,
            compresslevel=9,
        ) as zf:
            for member_path in control_members:
                _write_bytes(zf, member_path, control_data[member_path])
            for source, relative_path, expected_hash, expected_size in sources:
                _write_source(
                    zf,
                    source,
                    f"files/{relative_path}",
                    expected_hash,
                    expected_size,
                )
        temp_path.replace(output_path)
    except Exception:
        temp_path.unlink(missing_ok=True)
        raise
