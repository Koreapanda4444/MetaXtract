from __future__ import annotations

import json
from dataclasses import asdict, is_dataclass
from pathlib import Path
from typing import Any, Dict, Iterable, List

from .files import PathLike


JsonObj = Dict[str, Any]


def dumps_json(obj: Any) -> str:
    if is_dataclass(obj):
        obj = asdict(obj)
    return json.dumps(obj, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def write_jsonl(path: PathLike, rows: Iterable[Any]) -> None:
    output = Path(path)
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("w", encoding="utf-8") as stream:
        for row in rows:
            stream.write(dumps_json(row))
            stream.write("\n")


def read_jsonl(path: PathLike) -> List[JsonObj]:
    rows: List[JsonObj] = []
    with Path(path).open("r", encoding="utf-8") as stream:
        for line in stream:
            line = line.strip()
            if line:
                rows.append(json.loads(line))
    return rows
