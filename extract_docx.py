from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any, Optional


@dataclass(frozen=True)
class DocxExtractResult:
    ok: bool
    data: dict[str, Any]
    error_code: Optional[str] = None


def _to_iso(dt: Any) -> Optional[str]:
    if dt is None:
        return None
    if isinstance(dt, datetime):
        try:
            return dt.isoformat()
        except Exception:
            return None
    try:
        return datetime.fromisoformat(str(dt)).isoformat()
    except Exception:
        return None


def extract_docx_metadata(path: Path) -> DocxExtractResult:
    try:
        from docx import Document
    except Exception:
        return DocxExtractResult(ok=False, data={}, error_code="missing_dependency")

    try:
        doc = Document(str(path))
        cp = getattr(doc, "core_properties", None)
        if cp is None:
            return DocxExtractResult(ok=False, data={}, error_code="no_docx_meta")

        docx_out: dict[str, Any] = {}

        creator = getattr(cp, "author", None)
        last_modified_by = getattr(cp, "last_modified_by", None)
        title = getattr(cp, "title", None)
        created = getattr(cp, "created", None)
        modified = getattr(cp, "modified", None)

        if creator is not None:
            docx_out["creator"] = creator
        if last_modified_by is not None:
            docx_out["lastModifiedBy"] = last_modified_by
        if title is not None:
            docx_out["title"] = title
        if created is not None:
            docx_out["created"] = created
        if modified is not None:
            docx_out["modified"] = modified

        if not docx_out:
            return DocxExtractResult(ok=False, data={}, error_code="no_docx_meta")

        data: dict[str, Any] = {"docx": docx_out}

        times: dict[str, Any] = {}
        created_iso = _to_iso(created)
        modified_iso = _to_iso(modified)
        if created_iso:
            times["created"] = created_iso
        if modified_iso:
            times["modified"] = modified_iso
        if times:
            data["docx_times"] = times

        return DocxExtractResult(ok=True, data=data)

    except OSError:
        return DocxExtractResult(ok=False, data={}, error_code="read_error")
    except Exception:
        return DocxExtractResult(ok=False, data={}, error_code="extract_error")
