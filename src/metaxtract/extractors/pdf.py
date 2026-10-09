from __future__ import annotations

from typing import Any, Dict, List, Tuple

from pypdf import PdfReader

from .normalize import json_safe, normalize_metadata
from ..core.files import PathLike


def _metadata_value(info: Any, attribute: str, raw_key: str) -> Any:
    try:
        value = getattr(info, attribute, None)
    except Exception:
        value = None
    if value is not None:
        return value
    try:
        return info.get(raw_key)
    except (AttributeError, KeyError, TypeError):
        return None


def extract_pdf(path: PathLike) -> Tuple[Dict[str, Any], List[str]]:
    warnings: List[str] = []
    metadata: Dict[str, Any] = {}

    reader = PdfReader(str(path))
    encrypted = bool(reader.is_encrypted)
    metadata["pdf_encrypted"] = encrypted
    if encrypted:
        try:
            unlocked = bool(reader.decrypt(""))
        except Exception:
            unlocked = False
        if not unlocked:
            warnings.append("pdf_password_required")
            return normalize_metadata(metadata), warnings

    metadata["pages"] = len(reader.pages)

    try:
        info = reader.metadata
    except Exception:
        info = None

    if info:
        text_fields = {
            "/Title": "pdf_title",
            "/Author": "pdf_author",
            "/Subject": "pdf_subject",
            "/Creator": "pdf_creator",
            "/Producer": "pdf_producer",
            "/Keywords": "pdf_keywords",
        }
        for raw_key, output_key in text_fields.items():
            value = info.get(raw_key)
            if value not in (None, ""):
                metadata[output_key] = json_safe(value)

        creation_date = _metadata_value(info, "creation_date", "/CreationDate")
        modification_date = _metadata_value(info, "modification_date", "/ModDate")
        if creation_date is not None:
            metadata["pdf_creation_date"] = json_safe(creation_date)
        if modification_date is not None:
            metadata["pdf_modification_date"] = json_safe(modification_date)

    return normalize_metadata(metadata), warnings
