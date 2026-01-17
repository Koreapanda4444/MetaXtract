from __future__ import annotations

import fnmatch
import hashlib
import os
from pathlib import Path
from typing import Iterable, Optional


def normalize_extension(ext: str) -> str:
    e = (ext or "").strip().lower()
    if not e:
        return ""
    if not e.startswith("."):
        e = "." + e
    return e


def parse_include_extensions(raw: Optional[str]) -> Optional[set[str]]:
    if raw is None:
        return None
    text = raw.strip()
    if not text:
        return None
    parts = [p.strip() for p in text.split(",") if p.strip()]
    exts = {normalize_extension(p) for p in parts}
    exts.discard("")
    return exts or None


def split_patterns(patterns: Iterable[str]) -> list[str]:
    out: list[str] = []
    for p in patterns:
        if not p:
            continue
        for part in p.split(","):
            part = part.strip()
            if part:
                out.append(part)
    return out


def path_is_readable_file(path: Path) -> tuple[bool, Optional[str]]:
    try:
        if not path.exists():
            return False, f"경로가 존재하지 않습니다: {path}"
        if not path.is_file():
            return False, f"파일이 아닙니다: {path}"
        if not os.access(str(path), os.R_OK):
            return False, f"읽기 권한이 없습니다: {path}"
        return True, None
    except OSError as e:
        return False, f"파일 확인 중 오류: {path} ({e})"


def _is_glob_pattern(pattern: str) -> bool:
    return any(ch in pattern for ch in ["*", "?", "["])


def _normalize_pattern(pattern: str) -> str:
    p = (pattern or "").strip().replace("\\", "/").lower()
    if p.startswith("./"):
        p = p[2:]
    return p


def match_exclude(path_text: str, patterns: Iterable[str]) -> bool:
    text = path_text.lower()
    for raw in patterns:
        pat = _normalize_pattern(raw)
        if not pat:
            continue
        if _is_glob_pattern(pat):
            if fnmatch.fnmatch(text, pat.lower()):
                return True
            if pat.startswith("*/") and fnmatch.fnmatch(text, pat[2:]):
                return True
        else:
            if pat in text:
                return True
            if pat.startswith("*/") and pat[2:] in text:
                return True
    return False


def collect_stat(path: Path) -> dict:
    st = path.stat()
    return {
        "size_bytes": int(st.st_size),
        "os_times": {
            "atime": float(st.st_atime),
            "mtime": float(st.st_mtime),
            "ctime": float(st.st_ctime),
        },
    }


def compute_hash(path: Path, algo: str) -> Optional[str]:
    a = (algo or "").strip().lower()
    if a in {"", "none"}:
        return None
    if a not in {"sha256", "md5"}:
        raise ValueError(f"지원되지 않는 해시 알고리즘: {algo}")

    h = hashlib.new(a)
    with path.open("rb") as f:
        while True:
            chunk = f.read(1024 * 1024)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def make_record(path: Path, *, hash_algo: str = "none") -> dict:
    ext = path.suffix.lower()
    record: dict = {
        "path": str(path),
        "name": path.name,
        "ext": ext,
    }
    record.update(collect_stat(path))
    hh = compute_hash(path, hash_algo)
    if hh is not None:
        record["hash_algo"] = hash_algo
        record["hash_hex"] = hh
    return record
