from __future__ import annotations

import zipfile
from pathlib import Path

from report import build_report
from utils import PathLike, dumps_json, read_jsonl


def export_bundle(
    scan_jsonl_path: PathLike,
    out_zip_path: PathLike,
    *,
    report_json_name: str = "report.json",
    scan_jsonl_name: str = "scan.jsonl",
) -> None:
    scan_rows = read_jsonl(scan_jsonl_path)
    report = build_report(str(scan_jsonl_path))

    out = Path(out_zip_path)
    out.parent.mkdir(parents=True, exist_ok=True)

    with zipfile.ZipFile(out, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        zf.writestr(scan_jsonl_name, "\n".join(dumps_json(r) for r in scan_rows) + "\n")
        zf.writestr(report_json_name, dumps_json(report) + "\n")
