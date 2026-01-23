from __future__ import annotations

import html
from typing import Any, Dict

from report import build_report


def render_report_html(scan_jsonl_path: str) -> str:
    rep: Dict[str, Any] = build_report(scan_jsonl_path)

    def _li(k: str, v: Any) -> str:
        return f"<li><b>{html.escape(k)}</b>: {html.escape(str(v))}</li>"

    parts = [
        "<!doctype html>",
        "<html><head><meta charset='utf-8'><title>MetaXtract Report</title></head><body>",
        "<h1>MetaXtract Report</h1>",
        "<ul>",
        _li("total_files", rep.get("total_files")),
        "</ul>",
        "<h2>MIME counts</h2>",
        "<ul>",
    ]
    for k, v in sorted((rep.get("mime_counts") or {}).items()):
        parts.append(_li(k, v))
    parts.extend(["</ul>", "<h2>Warnings</h2>", "<ul>"])
    for k, v in sorted((rep.get("warning_counts") or {}).items()):
        parts.append(_li(k, v))
    parts.extend(["</ul>", "<h2>Errors</h2>", "<ul>"])
    for k, v in sorted((rep.get("error_counts") or {}).items()):
        parts.append(_li(k, v))
    parts.extend(["</ul>", "</body></html>"])
    return "\n".join(parts)
