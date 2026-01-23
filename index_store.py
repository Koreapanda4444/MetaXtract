from __future__ import annotations

from pathlib import Path
from typing import List, Optional

from schema import ScanRecord
from utils import PathLike, read_jsonl, write_jsonl


def save_index(path: PathLike, records: List[ScanRecord]) -> None:
    write_jsonl(path, records)


def load_index(path: PathLike) -> Optional[list]:
    p = Path(path)
    if not p.exists():
        return None
    return read_jsonl(p)
