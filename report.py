from __future__ import annotations

from collections import Counter
from typing import Any, Dict, List

from utils import JsonObj, read_jsonl


def build_report(scan_jsonl_path: str) -> Dict[str, Any]:
    rows: List[JsonObj] = read_jsonl(scan_jsonl_path)
    mime_counts = Counter(r.get("mime", "") for r in rows)
    warnings = Counter(w for r in rows for w in (r.get("warnings") or []))
    errors = Counter(e for r in rows for e in (r.get("errors") or []))

    return {
        "total_files": len(rows),
        "mime_counts": dict(mime_counts),
        "warning_counts": dict(warnings),
        "error_counts": dict(errors),
    }
