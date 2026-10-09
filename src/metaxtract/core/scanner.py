from __future__ import annotations

from pathlib import Path
from typing import Callable, Dict, List, Tuple

from ..config import Settings
from ..extractors.docx import extract_docx
from ..extractors.image import extract_image
from ..extractors.pdf import extract_pdf
from ..extractors.video import extract_video

from .cache import CacheStore
from .files import PathLike, get_relpath, guess_mime, iter_files, safe_stat, sha256_file
from .models import ScanRecord


Extractor = Callable[[PathLike], Tuple[Dict[str, object], List[str]]]
ProgressCallback = Callable[[int, int, str | None], None]
CancelCheck = Callable[[], bool]


class ScanCancelled(RuntimeError):
    """Raised when a caller requests cooperative scan cancellation."""


def _check_cancelled(cancel_check: CancelCheck | None) -> None:
    if cancel_check is not None and cancel_check():
        raise ScanCancelled("scan cancelled")


def _select_extractor(mime: str, path: PathLike) -> Extractor:
    ext = Path(path).suffix.lower()
    if mime.startswith("image/") or ext in {".jpg", ".jpeg", ".png"}:
        return extract_image
    if mime == "application/pdf" or ext == ".pdf":
        return extract_pdf
    if ext == ".docx":
        return extract_docx
    if mime.startswith("video/") or ext in {".mp4", ".mov", ".m4v"}:
        return extract_video
    return lambda _p: ({}, [])


def _validate_scan_file(
    path: Path,
    base: PathLike | None,
) -> tuple[Path, Path | None]:
    if path.is_symlink():
        raise ValueError(f"symbolic links are not supported: {path}")

    resolved_path = path.resolve(strict=False)
    if base is None:
        return path, None

    base_path = Path(base)
    if base_path.is_symlink():
        raise ValueError(f"symbolic links are not supported: {base_path}")
    resolved_base = base_path.resolve(strict=False)
    if resolved_path != resolved_base and not resolved_path.is_relative_to(resolved_base):
        raise ValueError(f"path escapes scan root: {path}")
    return resolved_path, resolved_base


def scan_file(
    path: PathLike,
    base: PathLike | None = None,
    cache: CacheStore = None,
    cache_mode: str = "sha256",
    cache_enabled: bool = True,
    defer_cache_write: bool = False,
) -> ScanRecord:
    p, resolved_base = _validate_scan_file(Path(path), base)
    mime = guess_mime(p)
    record_path = get_relpath(p, resolved_base)

    try:
        st = safe_stat(p)
    except OSError as exc:
        return ScanRecord(
            path=record_path,
            mime=mime,
            size_bytes=0,
            sha256="",
            metadata={},
            warnings=[],
            errors=[f"stat_failed:{type(exc).__name__}"],
        )

    warnings: List[str] = []
    errors: List[str] = []
    md: Dict[str, object] = {}
    sha: str | None = None

    if cache_enabled and cache is not None and cache_mode == "sha256":
        try:
            sha = sha256_file(p)
        except Exception as exc:
            errors.append(f"hash_failed:{type(exc).__name__}")

    if cache_enabled and cache is not None and not errors:
        try:
            cached = cache.get(
                p,
                mode=cache_mode,
                content_sha256=sha,
                file_stat=st,
            )
        except OSError:
            cached = None
        if cached is not None:
            metadata = dict(cached.get("metadata") or {})
            metadata["mtime"] = st["mtime"]
            metadata["cache_hit"] = True
            return ScanRecord(
                path=record_path,
                mime=str(cached.get("mime") or mime),
                size_bytes=st["size_bytes"],
                sha256=str(cached.get("sha256") or ""),
                metadata=metadata,
                warnings=list(cached.get("warnings") or []),
                errors=list(cached.get("errors") or []),
            )

    if sha is None and not errors:
        try:
            sha = sha256_file(p)
        except Exception as exc:
            errors.append(f"hash_failed:{type(exc).__name__}")
    sha = sha or ""

    try:
        extractor = _select_extractor(mime, p)
        md, w = extractor(p)
        warnings.extend(w)
    except Exception as exc:
        errors.append(f"extract_failed:{type(exc).__name__}")

    md = dict(md)
    md["mtime"] = st["mtime"]

    rec = ScanRecord(
        path=record_path,
        mime=mime,
        size_bytes=st["size_bytes"],
        sha256=sha,
        metadata=md,
        warnings=warnings,
        errors=errors,
    )
    if cache_enabled and cache is not None and not errors:
        try:
            cache.set(
                str(p),
                rec.model_dump() if hasattr(rec, "model_dump") else rec.__dict__,
                mode=cache_mode,
                content_sha256=sha,
                file_stat=st,
                flush=not defer_cache_write,
            )
        except OSError:
            pass
    return rec


def scan_path(
    root: PathLike,
    cache: CacheStore = None,
    cache_mode: str = "sha256",
    cache_enabled: bool = True,
    max_files: int = Settings.max_files,
    include_hidden: bool = Settings.include_hidden,
    progress_callback: ProgressCallback | None = None,
    cancel_check: CancelCheck | None = None,
) -> List[ScanRecord]:
    target = Path(root)
    if target.is_symlink():
        raise ValueError(f"symbolic links are not supported: {target}")
    if not target.exists():
        raise FileNotFoundError(f"target does not exist: {target}")
    if not target.is_file() and not target.is_dir():
        raise NotADirectoryError(f"target is not a regular file or directory: {target}")

    if max_files < 1:
        raise ValueError("max_files must be at least 1")

    target = target.resolve(strict=True)
    base = target.parent if target.is_file() else target
    exclude_paths = [cache.cache_dir] if cache is not None else []
    files = []
    for path in iter_files(
        target,
        include_hidden=include_hidden,
        max_files=max_files + 1,
        exclude_paths=exclude_paths,
    ):
        _check_cancelled(cancel_check)
        files.append(path)
    if len(files) > max_files:
        raise ValueError(f"file limit exceeded: more than {max_files} files")
    _check_cancelled(cancel_check)
    if progress_callback is not None:
        progress_callback(0, len(files), None)

    records = []
    try:
        for completed, path in enumerate(files, start=1):
            _check_cancelled(cancel_check)
            record = scan_file(
                path,
                base=base,
                cache=cache,
                cache_mode=cache_mode,
                cache_enabled=cache_enabled,
                defer_cache_write=True,
            )
            records.append(record)
            if progress_callback is not None:
                progress_callback(completed, len(files), record.path)
            _check_cancelled(cancel_check)
    finally:
        if cache_enabled and cache is not None:
            try:
                cache.flush()
            except OSError:
                pass
    records.sort(key=lambda r: r.path)
    return records
