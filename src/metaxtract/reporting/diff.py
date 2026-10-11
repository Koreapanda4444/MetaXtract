from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict, Iterable, List

from ..core.jsonio import JsonObj, read_jsonl


@dataclass(frozen=True)
class DiffSummary:
    added: List[str]
    removed: List[str]
    changed: List[str]
    field_changes: Dict[str, List[FieldChange]]


@dataclass(frozen=True)
class FieldChange:
    field: str
    before: Any
    after: Any
    before_present: bool = True
    after_present: bool = True


def _index_by_path(rows: List[JsonObj]) -> Dict[str, JsonObj]:
    return {r.get("path", ""): r for r in rows}


_MISSING = object()


def _compare_values(before: Any, after: Any, prefix: str) -> List[FieldChange]:
    if isinstance(before, dict) and isinstance(after, dict):
        changes = []
        for key in sorted(set(before) | set(after), key=str):
            field = f"{prefix}.{key}" if prefix else str(key)
            changes.extend(
                _compare_values(
                    before.get(key, _MISSING),
                    after.get(key, _MISSING),
                    field,
                )
            )
        return changes

    if before is not _MISSING and after is not _MISSING and before == after:
        return []
    return [
        FieldChange(
            field=prefix,
            before=None if before is _MISSING else before,
            after=None if after is _MISSING else after,
            before_present=before is not _MISSING,
            after_present=after is not _MISSING,
        )
    ]


def compare_records(before: JsonObj, after: JsonObj) -> List[FieldChange]:
    changes = []
    for field in sorted((set(before) | set(after)) - {"path"}):
        changes.extend(
            _compare_values(
                before.get(field, _MISSING),
                after.get(field, _MISSING),
                field,
            )
        )
    return changes


def diff_records(
    old_rows: Iterable[JsonObj],
    new_rows: Iterable[JsonObj],
) -> DiffSummary:
    old_idx = _index_by_path(list(old_rows))
    new_idx = _index_by_path(list(new_rows))

    old_keys = set(old_idx.keys())
    new_keys = set(new_idx.keys())

    added = sorted(k for k in new_keys - old_keys if k)
    removed = sorted(k for k in old_keys - new_keys if k)

    field_changes = {}
    for path in sorted(old_keys & new_keys):
        changes = compare_records(old_idx[path], new_idx[path])
        if changes:
            field_changes[path] = changes

    return DiffSummary(
        added=added,
        removed=removed,
        changed=list(field_changes),
        field_changes=field_changes,
    )


def diff_jsonl(old_path: str, new_path: str) -> DiffSummary:
    old_rows = read_jsonl(old_path)
    new_rows = read_jsonl(new_path)

    return diff_records(old_rows, new_rows)
