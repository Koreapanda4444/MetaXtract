from __future__ import annotations

import json
import hashlib
from pathlib import Path
from typing import Any, Iterable, Iterator, Optional


def write_jsonl(path: str | Path, records: Iterable[dict], *, append: bool = False) -> None:
    p = Path(path)
    p.parent.mkdir(parents=True, exist_ok=True)
    mode = "a" if append else "w"
    with p.open(mode, encoding="utf-8", newline="\n") as f:
        for rec in records:
            f.write(json.dumps(rec, ensure_ascii=False) + "\n")


def append_jsonl(path: str | Path, record: dict) -> None:
    write_jsonl(path, [record], append=True)


def iter_jsonl(path: str | Path) -> Iterator[dict]:
    p = Path(path)
    with p.open("r", encoding="utf-8") as f:
        for line in f:
            s = line.strip()
            if s and s[0] == "\ufeff":
                s = s.lstrip("\ufeff")
            if not s:
                continue
            yield json.loads(s)


def iter_jsonl_lines(path: str | Path) -> Iterator[tuple[int, str]]:
    """JSONL 파일을 라인 단위로 순회합니다.

    - 빈 줄은 스킵
    - BOM은 제거
    - json.loads는 호출하지 않음(검증/에러 수집 용도)
    """
    p = Path(path)
    with p.open("r", encoding="utf-8") as f:
        for line_no, line in enumerate(f, start=1):
            s = line.strip()
            if s and s[0] == "\ufeff":
                s = s.lstrip("\ufeff")
            if not s:
                continue
            yield line_no, s


def compute_file_digest(path: str | Path, *, algo: str = "sha256") -> str:
    a = (algo or "").strip().lower()
    if a not in {"sha256"}:
        raise ValueError(f"지원되지 않는 digest 알고리즘: {algo}")

    h = hashlib.new(a)
    p = Path(path)
    with p.open("rb") as f:
        while True:
            chunk = f.read(1024 * 1024)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def load_jsonl(path: str | Path, *, limit: Optional[int] = None) -> list[dict]:
    out: list[dict] = []
    for rec in iter_jsonl(path):
        out.append(rec)
        if limit is not None and len(out) >= limit:
            break
    return out


def get_session_header(path: str | Path) -> dict[str, Any]:
    it = iter_jsonl(path)
    first = next(it)
    if not isinstance(first, dict):
        raise ValueError("invalid session header")
    return first


def split_session_and_records(path: str | Path) -> tuple[dict[str, Any] | None, list[dict]]:
    items = load_jsonl(path)
    if not items:
        return None, []

    first = items[0]
    if isinstance(first, dict) and first.get("type") == "session":
        return first, [r for r in items[1:] if isinstance(r, dict)]
    return None, [r for r in items if isinstance(r, dict)]
