from __future__ import annotations

import re
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Optional


@dataclass(frozen=True)
class PdfExtractResult:
    ok: bool
    data: dict[str, Any]
    error_code: Optional[str] = None
    message_short: Optional[str] = None


def _short_message(text: Any, *, limit: int = 160) -> str:
    try:
        msg = str(text).strip()
    except Exception:
        msg = ""
    if not msg:
        msg = "error"
    if len(msg) > limit:
        msg = msg[: max(0, limit - 1)] + "…"
    return msg


_PDF_DATE_RE = re.compile(
    r"^D:(?P<y>\d{4})"
    r"(?P<m>\d{2})?"
    r"(?P<d>\d{2})?"
    r"(?P<h>\d{2})?"
    r"(?P<min>\d{2})?"
    r"(?P<s>\d{2})?"
    r"(?P<tz>Z|[+-]\d{2}'?\d{2}'?)?$"
)


def _parse_pdf_date(value: Any) -> Optional[str]:
    if value is None:
        return None
    text = str(value).strip()
    if not text:
        return None

    m = _PDF_DATE_RE.match(text)
    if not m:
        return None

    y = int(m.group("y"))
    mo = int(m.group("m") or 1)
    d = int(m.group("d") or 1)
    h = int(m.group("h") or 0)
    mi = int(m.group("min") or 0)
    s = int(m.group("s") or 0)

    tz = m.group("tz")
    tzinfo = None
    if tz:
        if tz == "Z":
            tzinfo = timezone.utc
        else:
            sign = 1 if tz[0] == "+" else -1
            hh = int(tz[1:3])
            mm = int(tz[-2:])
            tzinfo = timezone(sign * timedelta(hours=hh, minutes=mm))

    try:
        dt = datetime(y, mo, d, h, mi, s, tzinfo=tzinfo)
        return dt.isoformat()
    except Exception:
        return None


def extract_pdf_metadata(path: Path) -> PdfExtractResult:
    try:
        from pypdf import PdfReader
    except Exception:
        return PdfExtractResult(ok=False, data={}, error_code="missing_dependency", message_short="pypdf가 필요합니다")

    try:
        reader = PdfReader(str(path))
        meta = getattr(reader, "metadata", None)

        data: dict[str, Any] = {}
        pdf_out: dict[str, Any] = {}

        if meta:
            for key in ("/Author", "/Creator", "/Producer", "/Title", "/CreationDate", "/ModDate"):
                try:
                    v = meta.get(key)
                except Exception:
                    v = None
                if v is not None:
                    pdf_out[key.lstrip("/")] = v

        if not pdf_out:
            return PdfExtractResult(ok=False, data=data, error_code="no_pdf_meta", message_short="PDF 메타 없음")

        data["pdf"] = pdf_out

        times: dict[str, Any] = {}
        created = _parse_pdf_date(pdf_out.get("CreationDate"))
        modified = _parse_pdf_date(pdf_out.get("ModDate"))
        if created:
            times["CreationDate"] = created
        if modified:
            times["ModDate"] = modified
        if times:
            data["pdf_times"] = times

        return PdfExtractResult(ok=True, data=data)
    except OSError:
        return PdfExtractResult(ok=False, data={}, error_code="read_error", message_short="읽기 실패")
    except Exception as e:
        return PdfExtractResult(ok=False, data={}, error_code="extract_error", message_short=_short_message(e))
