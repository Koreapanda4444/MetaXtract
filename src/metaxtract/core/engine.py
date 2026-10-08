from __future__ import annotations

from pathlib import Path
from typing import Callable, Dict, List, Tuple

from ..config import Settings
from ..extractors.docx import extract_docx
from ..extractors.image import extract_image
from ..extractors.pdf import extract_pdf
from ..extractors.video import extract_video

from .schema import ScanRecord
from .utils import PathLike, get_relpath, guess_mime, iter_files, safe_stat, sha256_file
from .cache import CacheStore


Extractor = Callable[[PathLike], Tuple[Dict[str, object], List[str]]]


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


def scan_file(
    path: PathLike,
    base: PathLike | None = None,
    cache: CacheStore = None,
    cache_mode: str = "sha256",
    cache_enabled: bool = True,
) -> ScanRecord:
    p = Path(path)
    mime = guess_mime(p)
    record_path = get_relpath(p, base)

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

    if cache_enabled and cache is not None:
        try:
            cached = cache.get(str(p), mode=cache_mode)
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

    warnings: List[str] = []
    errors: List[str] = []
    md: Dict[str, object] = {}

    try:
        sha = sha256_file(p)
    except Exception as e:
        sha = ""
        errors.append(f"hash_failed:{type(e).__name__}")

    try:
        extractor = _select_extractor(mime, p)
        md, w = extractor(p)
        warnings.extend(w)
    except Exception as e:
        errors.append(f"extract_failed:{type(e).__name__}")

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
) -> List[ScanRecord]:
    target = Path(root)
    if not target.exists():
        raise FileNotFoundError(f"target does not exist: {target}")
    if not target.is_file() and not target.is_dir():
        raise NotADirectoryError(f"target is not a regular file or directory: {target}")

    if max_files < 1:
        raise ValueError("max_files must be at least 1")

    base = target.parent if target.is_file() else target
    exclude_paths = [cache.cache_dir] if cache is not None else []
    files = list(
        iter_files(
            target,
            include_hidden=include_hidden,
            max_files=max_files + 1,
            exclude_paths=exclude_paths,
        )
    )
    if len(files) > max_files:
        raise ValueError(f"file limit exceeded: more than {max_files} files")
    records = [
        scan_file(
            p,
            base=base,
            cache=cache,
            cache_mode=cache_mode,
            cache_enabled=cache_enabled
        )
        for p in files
    ]
    records.sort(key=lambda r: r.path)
    return records
