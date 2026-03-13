from __future__ import annotations

import zipfile
from pathlib import Path


import json
from report import build_report
from utils import PathLike, dumps_json, read_jsonl
from manifest import build_manifest


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
    report = build_report(str(scan_jsonl_path))
    hashes = []
    for r in scan_rows:
        hashes.append(f"{r.get('sha256', '')}\t{r.get('path', '')}")

    manifest_opts = {
        "case_id": case_id,
        "notes": notes,
        "hashes": [r.get("sha256", "") for r in scan_rows],
        "redacted": redact,
    }
    manifest = build_manifest(scan_rows, manifest_opts)

    out = Path(out_zip_path)
    out.parent.mkdir(parents=True, exist_ok=True)

    with zipfile.ZipFile(out, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        zf.writestr("manifest.json", json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
        zf.writestr("scan.jsonl", "\n".join(dumps_json(r) for r in scan_rows) + "\n")
        zf.writestr("hashes.txt", "\n".join(hashes) + "\n")
        zf.writestr("reports/report.json", dumps_json(report) + "\n")
        # 향후 html/csv 등 추가 가능
        if include_files and files_base:
            base = Path(files_base)
            for r in scan_rows:
                rel = r.get("path")
                if not rel:
                    continue
                src = base / rel
                if src.exists():
                    arcname = f"files/{rel}"
                    zf.write(src, arcname)
