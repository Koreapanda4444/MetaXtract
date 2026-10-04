from __future__ import annotations

import json
import zipfile
from pathlib import Path, PurePosixPath, PureWindowsPath

from manifest import build_manifest
from report import build_report_from_rows
from sanitize import sanitize_row
from utils import PathLike, dumps_json, read_jsonl


def _normalize_relative_path(value: object) -> str:
    if not isinstance(value, str) or not value:
        raise ValueError("bundle record has an invalid path")
    if any(char in value for char in "\x00\r\n\t"):
        raise ValueError(f"bundle path contains control characters: {value!r}")
    if PureWindowsPath(value).drive:
        raise ValueError(f"absolute bundle path is not allowed: {value}")

    normalized = value.replace("\\", "/")
    path = PurePosixPath(normalized)
    parts = normalized.split("/")
    if path.is_absolute() or any(part in {"", ".", ".."} for part in parts):
        raise ValueError(f"unsafe bundle path: {value}")
    return path.as_posix()


def _resolve_source(base: Path, relative_path: str) -> Path:
    source = base
    for part in PurePosixPath(relative_path).parts:
        source = source / part
        if source.is_symlink():
            raise ValueError(f"symbolic links are not allowed: {relative_path}")

    if not source.exists():
        raise FileNotFoundError(f"bundle source is missing: {relative_path}")
    if not source.is_file():
        raise IsADirectoryError(f"bundle source is not a file: {relative_path}")

    resolved = source.resolve(strict=True)
    if not resolved.is_relative_to(base):
        raise ValueError(f"bundle source escapes files base: {relative_path}")
    return resolved


def export_case_bundle(
    scan_jsonl_path: PathLike,
    out_zip_path: PathLike,
    *,
    include_files: bool = False,
    redact: bool = False,
    case_id: str = None,
    notes: str = None,
    files_base: PathLike = None,
) -> None:
    """케이스 번들(zip) 생성: manifest, scan, hashes, report, (옵션)원본파일 포함"""
    scan_rows = read_jsonl(scan_jsonl_path)
    if redact and include_files:
        raise ValueError("--redact cannot be combined with --include-files")

    candidate_rows = [sanitize_row(row) for row in scan_rows] if redact else scan_rows
    output_rows = []
    seen_paths = set()
    for row in candidate_rows:
        if not isinstance(row, dict):
            raise ValueError("bundle scan rows must be JSON objects")
        normalized_path = _normalize_relative_path(row.get("path"))
        collision_key = normalized_path.casefold()
        if collision_key in seen_paths:
            raise ValueError(f"duplicate bundle path: {normalized_path}")
        seen_paths.add(collision_key)
        normalized_row = dict(row)
        normalized_row["path"] = normalized_path
        output_rows.append(normalized_row)

    sources = []
    if include_files:
        base = Path(files_base) if files_base is not None else Path(scan_jsonl_path).parent
        if not base.exists():
            raise FileNotFoundError(f"files base does not exist: {base}")
        if not base.is_dir():
            raise NotADirectoryError(f"files base is not a directory: {base}")
        base = base.resolve(strict=True)
        sources = [
            (_resolve_source(base, row["path"]), row["path"])
            for row in output_rows
        ]

    report = build_report_from_rows(output_rows)
    hashes = []
    for r in output_rows:
        hashes.append(f"{r.get('sha256', '')}\t{r.get('path', '')}")

    manifest_opts = {
        "case_id": case_id,
        "notes": notes,
        "hashes": [r.get("sha256", "") for r in output_rows],
        "redacted": redact,
    }
    manifest = build_manifest(output_rows, manifest_opts)

    out = Path(out_zip_path)
    out.parent.mkdir(parents=True, exist_ok=True)

    with zipfile.ZipFile(out, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        zf.writestr("manifest.json", json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
        zf.writestr("scan.jsonl", "\n".join(dumps_json(r) for r in output_rows) + "\n")
        zf.writestr("hashes.txt", "\n".join(hashes) + "\n")
        zf.writestr("reports/report.json", dumps_json(report) + "\n")
        for source, relative_path in sources:
            zf.write(source, f"files/{relative_path}")
