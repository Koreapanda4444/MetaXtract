from __future__ import annotations

from typing import Any, Dict, List, Tuple

from PyPDF2 import PdfReader

from utils import PathLike


def extract_pdf(path: PathLike) -> Tuple[Dict[str, Any], List[str]]:
    warnings: List[str] = []
    md: Dict[str, Any] = {}

    reader = PdfReader(str(path))
    md["pages"] = len(reader.pages)

    try:
        info = reader.metadata
    except Exception:
        info = None

    if info:
        # PyPDF2 metadata values can be indirect or weird; coerce to strings.
        for key in ("/Title", "/Author", "/Subject", "/Creator", "/Producer"):
            if key in info and info[key]:
                md[f"pdf_{key.strip('/').lower()}"] = str(info[key])

    return md, warnings
