
import html
from collections import Counter
from typing import Any, Dict, List


def render_html(records: List[Dict[str, Any]]) -> str:
    def _li(k: str, v: Any) -> str:
        return f"<li><b>{html.escape(str(k))}</b>: {html.escape(str(v))}</li>"

    # 파일 요약
    total_files = len(records)
    total_size = sum(r.get("size_bytes", 0) for r in records)
    mime_counts = Counter(r.get("mime", "") for r in records)

    # 주요 발견사항 예시: GPS, 작성자, 편집툴, 타임라인 이상
    gps_files = [r for r in records if "gps" in (r.get("metadata") or {})]
    authors = Counter(
        (r.get("metadata") or {}).get("author")
        for r in records
        if (r.get("metadata") or {}).get("author")
    )
    producers = Counter(
        (r.get("metadata") or {}).get("producer")
        for r in records
        if (r.get("metadata") or {}).get("producer")
    )
    models = Counter(
        (r.get("metadata") or {}).get("model")
        for r in records
        if (r.get("metadata") or {}).get("model")
    )

    warnings = []
    errors = []
    for r in records:
        warnings.extend(r.get("warnings") or [])
        errors.extend(r.get("errors") or [])
    warning_counts = Counter(warnings)
    error_counts = Counter(errors)

    parts = [
        "<!doctype html>",
        (
            "<html><head><meta charset='utf-8'><title>MetaXtract Report</title>"
            "<style>body{font-family:sans-serif;} .warn{color:#b85c00;} "
            ".err{color:#b80000;font-weight:bold;} "
            "table{border-collapse:collapse;} td,th{border:1px solid #ccc;padding:4px;}"
            "</style></head><body>"
        ),
        "<h1>MetaXtract Report</h1>",
        "<h2>요약</h2>",
        (
            f"<ul><li>파일 개수: <b>{total_files}</b></li>"
            f"<li>총 용량: <b>{total_size:,} bytes</b></li></ul>"
        ),
        "<h3>파일 타입별 개수</h3>",
        "<ul>"
    ]
    for k, v in sorted(mime_counts.items()):
        parts.append(_li(k, v))
    parts.append("</ul>")

    # 주요 발견사항
    parts.append("<h2>주요 발견사항</h2><ul>")
    if gps_files:
        parts.append(f"<li>GPS 정보 포함 파일: <b>{len(gps_files)}</b></li>")
    if authors:
        parts.append(f"<li>작성자(author) 정보: {len(authors)}명</li>")
    if producers:
        parts.append(f"<li>Producer/Software: {len(producers)}종</li>")
    if models:
        parts.append(f"<li>촬영 기기 모델: {len(models)}종</li>")
    if not (gps_files or authors or producers or models):
        parts.append("<li>특이 발견사항 없음</li>")
    parts.append("</ul>")

    # Top N
    def top_n(counter, title, n=5):
        if not counter:
            return ""
        s = f"<h3>Top {n} {title}</h3><ol>"
        for k, v in counter.most_common(n):
            s += f"<li>{html.escape(str(k))}: {v}</li>"
        s += "</ol>"
        return s

    parts.append(top_n(authors, "작성자"))
    parts.append(top_n(producers, "Producer/Software"))
    parts.append(top_n(models, "기기 모델"))

    # Warnings/Errors
    parts.append("<h2 class='warn'>Warnings</h2>")
    if warning_counts:
        parts.append("<ul>")
        for k, v in warning_counts.most_common():
            parts.append(
                f"<li class='warn'>{html.escape(str(k))}: {v}</li>"
            )
        parts.append("</ul>")
    else:
        parts.append("<p>경고 없음</p>")

    parts.append("<h2 class='err'>Errors</h2>")
    if error_counts:
        parts.append("<ul>")
        for k, v in error_counts.most_common():
            parts.append(
                f"<li class='err'>{html.escape(str(k))}: {v}</li>"
            )
        parts.append("</ul>")
    else:
        parts.append("<p>오류 없음</p>")

    # (선택) 파일 리스트 테이블
    parts.append(
        "<h2>파일 목록</h2>"
        "<table><tr>"
        "<th>경로</th><th>MIME</th><th>크기</th>"
        "<th>경고</th><th>오류</th></tr>"
    )
    for r in records:
        parts.append(
            f"<tr><td>{html.escape(str(r.get('path', '')))}</td>"
            f"<td>{html.escape(str(r.get('mime', '')))}</td>"
            f"<td>{r.get('size_bytes', '')}</td>"
            f"<td>{'|'.join(map(str, r.get('warnings') or []))}</td>"
            f"<td>{'|'.join(map(str, r.get('errors') or []))}</td></tr>"
        )
    parts.append("</table>")

    parts.append("</body></html>")
    return "\n".join(parts)
