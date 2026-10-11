from __future__ import annotations

from pathlib import Path, PurePosixPath

from ..core.models import normalize_relative_path as normalize_relative_path


def resolve_source(base: Path, relative_path: str) -> Path:
    source = base
    for part in PurePosixPath(relative_path).parts:
        source = source / part
        if source.is_symlink():
            raise ValueError(f"symbolic links are not allowed: {relative_path}")

    if not source.exists():
        raise FileNotFoundError(f"source is missing: {relative_path}")
    if not source.is_file():
        raise IsADirectoryError(f"source is not a file: {relative_path}")

    resolved = source.resolve(strict=True)
    if not resolved.is_relative_to(base):
        raise ValueError(f"source escapes files base: {relative_path}")
    return resolved
