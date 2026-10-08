from __future__ import annotations

import hashlib
import json
import os
from dataclasses import asdict, is_dataclass
from pathlib import Path
from typing import Any, Dict, Iterable, Iterator, List, Optional, Union


JsonObj = Dict[str, Any]
PathLike = Union[str, os.PathLike[str]]


def sha256_file(path: PathLike, chunk_size: int = 1024 * 1024) -> str:
    h = hashlib.sha256()
    p = Path(path)
    with p.open("rb") as f:
        while True:
            chunk = f.read(chunk_size)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def safe_stat(path: PathLike) -> Dict[str, Any]:
    p = Path(path)
    st = p.stat()
    return {
        "size_bytes": int(st.st_size),
        "mtime": int(st.st_mtime),
    }


def dumps_json(obj: Any) -> str:
    if is_dataclass(obj):
        obj = asdict(obj)
    return json.dumps(obj, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def write_jsonl(path: PathLike, rows: Iterable[Any]) -> None:
    p = Path(path)
    p.parent.mkdir(parents=True, exist_ok=True)
    with p.open("w", encoding="utf-8") as f:
        for row in rows:
            f.write(dumps_json(row))
            f.write("\n")


def read_jsonl(path: PathLike) -> List[JsonObj]:
    p = Path(path)
    out: List[JsonObj] = []
    with p.open("r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            out.append(json.loads(line))
    return out


def iter_files(
    root: PathLike,
    *,
    include_hidden: bool = False,
    max_files: Optional[int] = None,
    exclude_paths: Iterable[PathLike] = (),
) -> Iterator[Path]:
    p = Path(root)
    if p.is_file():
        yield p
        return

    excluded_names = {".git", "__pycache__", ".metaxtract_cache"}
    excluded_paths = {Path(path).resolve() for path in exclude_paths}
    yielded = 0

    def is_excluded(path: Path) -> bool:
        resolved = path.resolve()
        return any(
            resolved == excluded or resolved.is_relative_to(excluded)
            for excluded in excluded_paths
        )

    for cur, dirs, files in os.walk(p):
        current = Path(cur)
        dirs[:] = sorted(
            name
            for name in dirs
            if name not in excluded_names
            and (include_hidden or not name.startswith("."))
            and not is_excluded(current / name)
        )
        for name in sorted(files):
            if not include_hidden and name.startswith("."):
                continue
            path = current / name
            if is_excluded(path):
                continue
            yield path
            yielded += 1
            if max_files is not None and yielded >= max_files:
                return


def guess_mime(path: PathLike) -> str:
    p = Path(path)
    ext = p.suffix.lower()
    if ext in {".jpg", ".jpeg"}:
        return "image/jpeg"
    if ext == ".png":
        return "image/png"
    if ext == ".pdf":
        return "application/pdf"
    if ext == ".docx":
        return "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    if ext in {".mp4", ".mov", ".m4v"}:
        return "video/mp4"
    return "application/octet-stream"


def get_relpath(path: PathLike, base: Optional[PathLike]) -> str:
    p = Path(path)
    if base is None:
        return str(p)
    try:
        return str(p.relative_to(Path(base)))
    except Exception:
        return str(p)
