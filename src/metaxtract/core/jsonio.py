from __future__ import annotations

import json
from dataclasses import asdict, is_dataclass
from pathlib import Path
from typing import Any, Dict, Iterable, List

from .files import PathLike
from .models import require_valid_records


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


def read_jsonl(path: PathLike, *, validate: bool = True) -> List[JsonObj]:
    rows: List[Any] = []
    with Path(path).open("r", encoding="utf-8") as stream:
        for line_number, line in enumerate(stream, start=1):
            line = line.strip()
            if line:
                try:
                    rows.append(json.loads(line))
                except json.JSONDecodeError as exc:
                    raise ValueError(
                        f"invalid JSONL at line {line_number}: {exc.msg}"
                    ) from exc
    if validate:
        return require_valid_records(rows)
    return rows
