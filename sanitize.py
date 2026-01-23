from __future__ import annotations

from typing import Iterable, List

from rules_privacy import PRIVACY_KEYS
from schema import ScanRecord


def sanitize_record(record: ScanRecord, remove_keys: Iterable[str] = PRIVACY_KEYS) -> ScanRecord:
    remove = set(remove_keys)
    md = {k: v for k, v in record.metadata.items() if k not in remove}
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
