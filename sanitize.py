from __future__ import annotations

from typing import Any, Dict, Iterable, List

from rules_privacy import PRIVACY_KEYS
from schema import ScanRecord


def sanitize_record(record: ScanRecord, remove_keys: Iterable[str] = PRIVACY_KEYS) -> ScanRecord:
    md = sanitize_metadata(record.metadata, remove_keys)
    return ScanRecord(
        path=record.path,
        mime=record.mime,
        size_bytes=record.size_bytes,
        sha256=record.sha256,
        metadata=md,
        warnings=list(record.warnings),
        errors=list(record.errors),
    )


def sanitize_records(records: List[ScanRecord]) -> List[ScanRecord]:
    return [sanitize_record(r) for r in records]


def sanitize_metadata(value: Any, remove_keys: Iterable[str] = PRIVACY_KEYS) -> Any:
    remove = set(remove_keys)
    if isinstance(value, dict):
        return {
            key: sanitize_metadata(item, remove)
            for key, item in value.items()
            if key not in remove
        }
    if isinstance(value, list):
        return [sanitize_metadata(item, remove) for item in value]
    return value


def sanitize_row(row: Dict[str, Any]) -> Dict[str, Any]:
    sanitized = dict(row)
    sanitized["metadata"] = sanitize_metadata(row.get("metadata") or {})
    return sanitized
