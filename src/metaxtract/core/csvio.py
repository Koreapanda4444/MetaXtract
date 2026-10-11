from __future__ import annotations

import csv
import os
import stat
import tempfile
from pathlib import Path
from typing import Any, Iterable, Mapping

from .files import PathLike
from .jsonio import dumps_json
from .models import require_valid_records


CSV_FIELDS = (
    "path",
    "mime",
    "size_bytes",
    "sha256",
    "status",
    "warnings",
    "errors",
    "note",
    "tags",
    "metadata_json",
)
FORMULA_PREFIXES = ("=", "+", "-", "@")


def _annotation_values(annotation: Any) -> tuple[str, list[str]]:
    if annotation is None:
        return "", []
    if isinstance(annotation, Mapping):
        note = annotation.get("note", "")
        tags = annotation.get("tags", ())
    else:
        note = getattr(annotation, "note", "")
        tags = getattr(annotation, "tags", ())
    return str(note or ""), [str(tag) for tag in (tags or ())]


def _status(record: Mapping[str, Any]) -> str:
    if record.get("errors"):
        return "Error"
    if record.get("warnings"):
        return "Warning"
    return "OK"


def _spreadsheet_safe(value: Any) -> Any:
    if not isinstance(value, str):
        return value
    if value.lstrip().startswith(FORMULA_PREFIXES):
        return "'" + value
    return value


def csv_rows(
    records: Iterable[Any],
    annotations: Mapping[str, Any] | None = None,
) -> list[dict[str, Any]]:
    valid_records = require_valid_records(records)
    annotation_index = annotations or {}
    rows = []
    for record in valid_records:
        note, tags = _annotation_values(annotation_index.get(record["path"]))
        row = {
            "path": record["path"],
            "mime": record["mime"],
            "size_bytes": record["size_bytes"],
            "sha256": record["sha256"],
            "status": _status(record),
            "warnings": dumps_json(record["warnings"]),
            "errors": dumps_json(record["errors"]),
            "note": note,
            "tags": dumps_json(tags),
            "metadata_json": dumps_json(record["metadata"]),
        }
        rows.append({key: _spreadsheet_safe(value) for key, value in row.items()})
    return rows


def write_csv(
    path: PathLike,
    records: Iterable[Any],
    annotations: Mapping[str, Any] | None = None,
) -> None:
    output = Path(path)
    rows = csv_rows(records, annotations)
    output.parent.mkdir(parents=True, exist_ok=True)
    output_mode = stat.S_IMODE(output.stat().st_mode) if output.exists() else 0o644
    temp_handle = tempfile.NamedTemporaryFile(
        mode="w",
        encoding="utf-8-sig",
        newline="",
        prefix=f".{output.name}.",
        suffix=".tmp",
        dir=output.parent,
        delete=False,
    )
    temp_path = Path(temp_handle.name)
    try:
        with temp_handle as stream:
            writer = csv.DictWriter(
                stream,
                fieldnames=CSV_FIELDS,
                lineterminator="\n",
            )
            writer.writeheader()
            writer.writerows(rows)
            stream.flush()
            os.fsync(stream.fileno())
        temp_path.chmod(output_mode)
        temp_path.replace(output)
    except Exception:
        temp_handle.close()
        temp_path.unlink(missing_ok=True)
        raise
