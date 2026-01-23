from __future__ import annotations

from typing import Any, Dict, List, Tuple

import docx  # python-docx

from utils import PathLike


def extract_docx(path: PathLike) -> Tuple[Dict[str, Any], List[str]]:
    warnings: List[str] = []
    md: Dict[str, Any] = {}

    document = docx.Document(str(path))
    props = document.core_properties

    md["docx_paragraphs"] = len(document.paragraphs)
    md["docx_title"] = props.title or ""
    md["docx_author"] = props.author or ""
    md["docx_last_modified_by"] = props.last_modified_by or ""
    md["docx_subject"] = props.subject or ""
    md["docx_keywords"] = props.keywords or ""

    return md, warnings
