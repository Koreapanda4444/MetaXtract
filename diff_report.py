from __future__ import annotations

from dataclasses import dataclass
from typing import Dict, List

from utils import JsonObj, read_jsonl


@dataclass(frozen=True)
class DiffSummary:
    added: List[str]
    removed: List[str]
    changed: List[str]


def _index_by_path(rows: List[JsonObj]) -> Dict[str, JsonObj]:
    return {r.get("path", ""): r for r in rows}


def diff_jsonl(old_path: str, new_path: str) -> DiffSummary:
    old_rows = read_jsonl(old_path)
    new_rows = read_jsonl(new_path)

    old_idx = _index_by_path(old_rows)
    new_idx = _index_by_path(new_rows)

    old_keys = set(old_idx.keys())
    new_keys = set(new_idx.keys())

    added = sorted(k for k in new_keys - old_keys if k)
    removed = sorted(k for k in old_keys - new_keys if k)

    changed: List[str] = []
    for k in sorted(old_keys & new_keys):
        if old_idx[k] != new_idx[k]:
            changed.append(k)

    return DiffSummary(added=added, removed=removed, changed=changed)
