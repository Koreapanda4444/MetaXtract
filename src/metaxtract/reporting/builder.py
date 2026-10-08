from __future__ import annotations

from collections import Counter
from typing import Any, Dict, List

from .findings import collect_findings
from ..core.utils import JsonObj, read_jsonl


def build_report(scan_jsonl_path: str) -> Dict[str, Any]:
    rows: List[JsonObj] = read_jsonl(scan_jsonl_path)
    return build_report_from_rows(rows)


def build_report_from_rows(rows: List[JsonObj]) -> Dict[str, Any]:
    mime_counts = Counter(r.get("mime", "") for r in rows)
    warnings = Counter(w for r in rows for w in (r.get("warnings") or []))
    errors = Counter(e for r in rows for e in (r.get("errors") or []))
    findings = collect_findings(rows)

    return {
        "total_files": len(rows),
        "mime_counts": dict(mime_counts),
        "warning_counts": dict(warnings),
        "error_counts": dict(errors),
        "findings": {
            "gps_files": findings["gps_files"],
            "authors": dict(findings["authors"]),
            "producers": dict(findings["producers"]),
            "models": dict(findings["models"]),
        },
    }
