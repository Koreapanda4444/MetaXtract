from __future__ import annotations

from collections import Counter
from typing import Any, Dict, Iterable, List


def _values(metadata: Dict[str, Any], keys: Iterable[str]) -> List[str]:
    values = []
    seen = set()
    for key in keys:
        value = metadata.get(key)
        if value in (None, ""):
            continue
        text = str(value)
        if text not in seen:
            values.append(text)
            seen.add(text)
    return values


def collect_findings(records: List[Dict[str, Any]]) -> Dict[str, Any]:
    gps_files = []
    authors = Counter()
    producers = Counter()
    models = Counter()

    for record in records:
        metadata = record.get("metadata") or {}
        has_coordinates = (
            metadata.get("gps_latitude") is not None
            and metadata.get("gps_longitude") is not None
        )
        if has_coordinates or metadata.get("gps") is not None:
            gps_files.append(str(record.get("path") or ""))

        authors.update(_values(metadata, ("author", "pdf_author", "docx_author")))
        producers.update(
            _values(metadata, ("producer", "pdf_producer", "pdf_creator", "software"))
        )
        models.update(_values(metadata, ("model", "exif_model")))

    return {
        "gps_files": gps_files,
        "authors": authors,
        "producers": producers,
        "models": models,
    }
