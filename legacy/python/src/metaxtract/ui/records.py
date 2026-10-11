from __future__ import annotations

from dataclasses import asdict, is_dataclass
from typing import Any, Mapping

from metaxtract.core.query import filter_query


FILTER_ALL = "All"
FILTER_VALUES = (FILTER_ALL, "OK", "Warning", "Error")


def record_dict(record: Any) -> dict[str, Any]:
    if is_dataclass(record):
        return asdict(record)
    if hasattr(record, "model_dump"):
        return dict(record.model_dump())
    return dict(record)


def record_status(record: dict[str, Any]) -> str:
    if record.get("errors"):
        return "Error"
    if record.get("warnings"):
        return "Warning"
    return "OK"


def filter_records(
    records: list[dict[str, Any]],
    query: str = "",
    status: str = FILTER_ALL,
    annotations: Mapping[str, Any] | None = None,
) -> list[dict[str, Any]]:
    return filter_query(
        records,
        query,
        status=status,
        annotations=annotations,
    )


def format_bytes(value: int) -> str:
    size = float(max(value, 0))
    units = ("B", "KiB", "MiB", "GiB", "TiB")
    for unit in units:
        if size < 1024 or unit == units[-1]:
            return f"{int(size)} {unit}" if unit == "B" else f"{size:.1f} {unit}"
        size /= 1024
    return f"{value} B"
