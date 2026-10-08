from __future__ import annotations

import json
import zipfile
from pathlib import Path

from .manifest import build_manifest
from .paths import normalize_relative_path, resolve_source
from .redaction import sanitize_row
from ..core.files import PathLike, sha256_file
from ..core.jsonio import dumps_json, read_jsonl
from ..reporting.builder import build_report_from_rows


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
        normalized_path = normalize_relative_path(row.get("path"))
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
            (resolve_source(base, row["path"]), row["path"], row.get("sha256"))
            for row in output_rows
        ]
        for source, relative_path, expected_hash in sources:
            if sha256_file(source) != expected_hash:
                raise ValueError(f"bundle source hash changed: {relative_path}")

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
        for source, relative_path, _expected_hash in sources:
            zf.write(source, f"files/{relative_path}")
