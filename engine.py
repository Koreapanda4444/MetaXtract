from __future__ import annotations

from pathlib import Path
from typing import Callable, Dict, List, Tuple

from extract_docx import extract_docx
from extract_image import extract_image
from extract_pdf import extract_pdf
from extract_video import extract_video
from schema import ScanRecord
from utils import PathLike, get_relpath, guess_mime, iter_files, safe_stat, sha256_file


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


def scan_file(path: PathLike, base: PathLike | None = None) -> ScanRecord:
    p = Path(path)
    st = safe_stat(p)
    mime = guess_mime(p)

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

    return ScanRecord(
        path=get_relpath(p, base),
        mime=mime,
        size_bytes=st["size_bytes"],
        sha256=sha,
        metadata=md,
        warnings=warnings,
        errors=errors,
    )


def scan_path(root: PathLike) -> List[ScanRecord]:
    base = Path(root)
    files = list(iter_files(base))
    records = [scan_file(p, base=base) for p in files]
    records.sort(key=lambda r: r.path)
    return records
