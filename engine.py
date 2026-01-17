from __future__ import annotations

import os
import platform
from datetime import datetime, timezone
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Optional

import extract_common
import normalize
import utils


@dataclass(frozen=True)
class EnumerationResult:
    files: list[str]
    found: int
    excluded: int
    errors: int
    error_messages: list[str]


@dataclass(frozen=True)
class ScanResult:
    records: list[dict]
    found: int
    excluded: int
    errors: int
    error_messages: list[str]


def make_session_header(
    *,
    root: str,
    recursive: bool,
    include: Optional[str],
    exclude: list[str],
    hash_algo: str,
) -> dict:
    return {
        "type": "session",
        "tool": {"name": "metaxtract", "version": utils.get_version()},
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "platform": {
            "system": platform.system(),
            "release": platform.release(),
            "version": platform.version(),
            "machine": platform.machine(),
            "python": platform.python_version(),
        },
        "scan": {
            "root": root,
            "recursive": bool(recursive),
            "include": include,
            "exclude": list(exclude),
            "hash": str(hash_algo),
        },
    }


def enumerate_files(
    root: str,
    *,
    recursive: bool,
    include_exts: Optional[set[str]] = None,
    exclude_patterns: Optional[Iterable[str]] = None,
) -> EnumerationResult:
    root_path = Path(root)
    if not root_path.exists():
        raise FileNotFoundError(f"경로가 존재하지 않습니다: {root}")

    patterns = list(exclude_patterns or [])

    candidates: list[Path] = []
    errors: list[str] = []

    if root_path.is_file():
        candidates = [root_path]
    elif root_path.is_dir():
        if recursive:
            def _onerror(e: OSError) -> None:
                errors.append(str(e))

            for base, _, files in os.walk(root_path, onerror=_onerror):
                base_path = Path(base)
                for name in files:
                    candidates.append(base_path / name)
        else:
            try:
                for entry in root_path.iterdir():
                    if entry.is_file():
                        candidates.append(entry)
            except OSError as e:
                errors.append(f"디렉터리 열거 중 오류: {root_path} ({e})")
    else:
        raise OSError(f"지원되지 않는 경로 타입입니다: {root}")

    found_files: list[str] = []
    excluded = 0

    base_for_rel = root_path if root_path.is_dir() else root_path.parent

    for p in candidates:
        ok, err = extract_common.path_is_readable_file(p)
        if not ok:
            errors.append(err or f"파일 접근 실패: {p}")
            continue

        ext = p.suffix.lower()
        if include_exts is not None and ext not in include_exts:
            excluded += 1
            continue

        try:
            rel = str(p.relative_to(base_for_rel))
        except Exception:
            rel = str(p)

        rel_norm = rel.replace("\\", "/")
        full_norm = str(p).replace("\\", "/")

        if extract_common.match_exclude(rel_norm, patterns) or extract_common.match_exclude(full_norm, patterns):
            excluded += 1
            continue

        found_files.append(str(p))

    return EnumerationResult(
        files=found_files,
        found=len(found_files),
        excluded=excluded,
        errors=len(errors),
        error_messages=errors,
    )


def scan(
    root: str,
    *,
    recursive: bool,
    include_exts: Optional[set[str]] = None,
    exclude_patterns: Optional[Iterable[str]] = None,
    hash_algo: str = "none",
) -> ScanResult:
    enum = enumerate_files(
        root,
        recursive=recursive,
        include_exts=include_exts,
        exclude_patterns=exclude_patterns,
    )

    records: list[dict] = []
    errors: list[str] = list(enum.error_messages)

    for p in enum.files:
        try:
            raw_record = extract_common.make_record(Path(p), hash_algo=hash_algo)
            records.append(normalize.normalize_record(raw_record))
        except OSError as e:
            errors.append(f"메타 수집 실패: {p} ({e})")
        except ValueError as e:
            errors.append(str(e))

    return ScanResult(
        records=records,
        found=enum.found,
        excluded=enum.excluded,
        errors=len(errors),
        error_messages=errors,
    )
