from __future__ import annotations

from pathlib import Path, PurePosixPath, PureWindowsPath


def normalize_relative_path(value: object) -> str:
    if not isinstance(value, str) or not value:
        raise ValueError("record has an invalid path")
    if any(char in value for char in "\x00\r\n\t"):
        raise ValueError(f"path contains control characters: {value!r}")
    if PureWindowsPath(value).drive:
        raise ValueError(f"absolute path is not allowed: {value}")

    normalized = value.replace("\\", "/")
    path = PurePosixPath(normalized)
    parts = normalized.split("/")
    if path.is_absolute() or any(part in {"", ".", ".."} for part in parts):
        raise ValueError(f"unsafe relative path: {value}")
    return path.as_posix()


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
