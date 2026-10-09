from __future__ import annotations

import hashlib
import os
from pathlib import Path
from typing import Any, Dict, Iterable, Iterator, Optional, Union


PathLike = Union[str, os.PathLike[str]]


def sha256_file(path: PathLike, chunk_size: int = 1024 * 1024) -> str:
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        while chunk := stream.read(chunk_size):
            digest.update(chunk)
    return digest.hexdigest()


def safe_stat(path: PathLike) -> Dict[str, Any]:
    st = Path(path).stat()
    return {
        "size_bytes": int(st.st_size),
        "mtime": int(st.st_mtime),
        "mtime_ns": int(st.st_mtime_ns),
    }


def _reject_symlink(path: Path) -> None:
    if path.is_symlink():
        raise ValueError(f"symbolic links are not supported: {path}")


def _resolve_inside(path: Path, root: Path) -> Path:
    try:
        resolved = path.resolve(strict=True)
    except OSError as exc:
        raise ValueError(f"cannot resolve scan path: {path}") from exc
    if resolved != root and not resolved.is_relative_to(root):
        raise ValueError(f"path escapes scan root: {path}")
    return resolved


def iter_files(
    root: PathLike,
    *,
    include_hidden: bool = False,
    max_files: Optional[int] = None,
    exclude_paths: Iterable[PathLike] = (),
) -> Iterator[Path]:
    source = Path(root)
    _reject_symlink(source)
    try:
        resolved_root = source.resolve(strict=True)
    except OSError as exc:
        raise FileNotFoundError(f"scan root does not exist: {source}") from exc

    if resolved_root.is_file():
        yield resolved_root
        return
    if not resolved_root.is_dir():
        raise NotADirectoryError(
            f"scan root is not a regular file or directory: {source}"
        )

    excluded_names = {".git", "__pycache__", ".metaxtract_cache"}
    excluded_paths = {Path(path).resolve(strict=False) for path in exclude_paths}
    yielded = 0

    def is_excluded(path: Path) -> bool:
        resolved = path.resolve(strict=False)
        return any(
            resolved == excluded or resolved.is_relative_to(excluded)
            for excluded in excluded_paths
        )

    for current_name, dirs, filenames in os.walk(
        resolved_root,
        followlinks=False,
    ):
        current = Path(current_name)
        kept_dirs = []
        for name in sorted(dirs):
            if name in excluded_names or (
                not include_hidden and name.startswith(".")
            ):
                continue
            candidate = current / name
            if is_excluded(candidate):
                continue
            _reject_symlink(candidate)
            _resolve_inside(candidate, resolved_root)
            kept_dirs.append(name)
        dirs[:] = kept_dirs

        for name in sorted(filenames):
            if not include_hidden and name.startswith("."):
                continue
            candidate = current / name
            if is_excluded(candidate):
                continue
            _reject_symlink(candidate)
            yield _resolve_inside(candidate, resolved_root)
            yielded += 1
            if max_files is not None and yielded >= max_files:
                return


def guess_mime(path: PathLike) -> str:
    ext = Path(path).suffix.lower()
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
    except ValueError:
        return str(p)
