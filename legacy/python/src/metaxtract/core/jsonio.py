from __future__ import annotations

import io
import json
import os
import stat
import tempfile
from dataclasses import asdict, is_dataclass
from pathlib import Path
from typing import Any, BinaryIO, Dict, Iterable, List

from ..config import MAX_JSONL_BYTES, MAX_JSONL_LINE_BYTES, MAX_JSONL_RECORDS
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
    output_mode = stat.S_IMODE(output.stat().st_mode) if output.exists() else 0o644
    temp_handle = tempfile.NamedTemporaryFile(
        mode="w",
        encoding="utf-8",
        newline="\n",
        prefix=f".{output.name}.",
        suffix=".tmp",
        dir=output.parent,
        delete=False,
    )
    temp_path = Path(temp_handle.name)
    try:
        with temp_handle as stream:
            for row in rows:
                stream.write(dumps_json(row))
                stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        temp_path.chmod(output_mode)
        temp_path.replace(output)
    except Exception:
        temp_handle.close()
        temp_path.unlink(missing_ok=True)
        raise


def _load_jsonl_stream(stream: BinaryIO, source: str) -> List[Any]:
    rows = []
    total_bytes = 0
    line_number = 0
    while True:
        raw_line = stream.readline(MAX_JSONL_LINE_BYTES + 1)
        if not raw_line:
            break
        line_number += 1
        total_bytes += len(raw_line)
        if len(raw_line) > MAX_JSONL_LINE_BYTES:
            raise ValueError(
                f"{source}: JSONL line {line_number} exceeds "
                f"{MAX_JSONL_LINE_BYTES} bytes"
            )
        if total_bytes > MAX_JSONL_BYTES:
            raise ValueError(f"{source}: JSONL exceeds {MAX_JSONL_BYTES} bytes")

        try:
            line = raw_line.decode("utf-8").strip()
        except UnicodeDecodeError as exc:
            raise ValueError(
                f"{source}: invalid UTF-8 at line {line_number}"
            ) from exc
        if not line:
            continue
        if len(rows) >= MAX_JSONL_RECORDS:
            raise ValueError(
                f"{source}: JSONL exceeds {MAX_JSONL_RECORDS} records"
            )
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError as exc:
            raise ValueError(
                f"{source}: invalid JSONL at line {line_number}: {exc.msg}"
            ) from exc
    return rows


def parse_jsonl_bytes(data: bytes, *, source: str = "JSONL") -> List[Any]:
    if len(data) > MAX_JSONL_BYTES:
        raise ValueError(f"{source}: JSONL exceeds {MAX_JSONL_BYTES} bytes")
    return _load_jsonl_stream(io.BytesIO(data), source)


def read_jsonl(path: PathLike, *, validate: bool = True) -> List[JsonObj]:
    input_path = Path(path)
    if input_path.stat().st_size > MAX_JSONL_BYTES:
        raise ValueError(f"{input_path}: JSONL exceeds {MAX_JSONL_BYTES} bytes")
    with input_path.open("rb") as stream:
        rows = _load_jsonl_stream(stream, str(input_path))
    if validate:
        return require_valid_records(rows)
    return rows
