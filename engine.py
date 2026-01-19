from __future__ import annotations

import concurrent.futures
import os
import platform
import threading
from datetime import datetime, timezone
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Iterable, Iterator, Optional

import extract_common
import extract_docx
import extract_image
import extract_pdf
import extract_video
import normalize
import rules_privacy
import rules_timeline
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
    threads: int = 1,
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
            "threads": int(threads),
        },
    }


def _process_one(
    path_text: str,
    *,
    base_for_rel: Path,
    hash_algo: str,
    redact: bool,
) -> dict:
    p_obj = Path(path_text)
    raw_record = extract_common.make_record(p_obj, hash_algo=hash_algo)

    # 디렉터리 스캔의 경우 path 키를 root 기준 상대경로로 고정합니다.
    # (sanitize로 outdir가 바뀌어도 diff --key path가 매칭되도록)
    try:
        rel = str(p_obj.relative_to(base_for_rel)).replace("\\", "/")
        raw_record["path"] = rel
    except Exception:
        raw_record["path"] = str(p_obj).replace("\\", "/")

    ext = str(raw_record.get("ext") or "").lower()

    def _mark_extract_failure(kind: str, code: str, msg: str) -> None:
        if code:
            # legacy compat
            raw_record["extract_error"] = code
            extract_common.add_error(
                raw_record,
                error_code=code,
                stage=f"extract.{kind}",
                message_short=(msg or code),
            )

    # extract by type
    if ext in {".jpg", ".jpeg", ".png"}:
        try:
            img_res = extract_image.extract_image_metadata(p_obj)
            raw_record.update(img_res.data)
            if not img_res.ok and img_res.error_code:
                _mark_extract_failure("image", img_res.error_code, getattr(img_res, "message_short", None) or "")
        except Exception as e:
            extract_common.add_error(
                raw_record,
                error_code="extract_error",
                stage="extract.image",
                message_short=utils.short_exc_message(e),
            )

    if ext == ".pdf":
        try:
            pdf_res = extract_pdf.extract_pdf_metadata(p_obj)
            raw_record.update(pdf_res.data)
            if not pdf_res.ok and pdf_res.error_code:
                _mark_extract_failure("pdf", pdf_res.error_code, getattr(pdf_res, "message_short", None) or "")
        except Exception as e:
            extract_common.add_error(
                raw_record,
                error_code="extract_error",
                stage="extract.pdf",
                message_short=utils.short_exc_message(e),
            )

    if ext == ".docx":
        try:
            docx_res = extract_docx.extract_docx_metadata(p_obj)
            raw_record.update(docx_res.data)
            if not docx_res.ok and docx_res.error_code:
                _mark_extract_failure("docx", docx_res.error_code, getattr(docx_res, "message_short", None) or "")
        except Exception as e:
            extract_common.add_error(
                raw_record,
                error_code="extract_error",
                stage="extract.docx",
                message_short=utils.short_exc_message(e),
            )

    if ext in {".mp4", ".m4v", ".mov"}:
        try:
            v_res = extract_video.extract_video_metadata(p_obj)
            raw_record.update(v_res.data)
            if not v_res.ok and v_res.error_code:
                _mark_extract_failure("video", v_res.error_code, getattr(v_res, "message_short", None) or "")
        except Exception as e:
            extract_common.add_error(
                raw_record,
                error_code="extract_error",
                stage="extract.video",
                message_short=utils.short_exc_message(e),
            )

    # normalize + rules (best-effort)
    try:
        normalized = normalize.normalize_record(raw_record)
    except Exception as e:
        extract_common.add_error(
            raw_record,
            error_code="normalize_error",
            stage="normalize",
            message_short=utils.short_exc_message(e),
        )
        normalized = normalize.normalize_record({"path": raw_record.get("path") or str(p_obj), "name": p_obj.name, "ext": ext, "size_bytes": int(raw_record.get("size_bytes") or 0), "os_times": raw_record.get("os_times") or {}, "errors": raw_record.get("errors") or []})
        normalized["raw"] = raw_record

    try:
        rules_privacy.apply_privacy_intelligence(normalized, redact=bool(redact))
    except Exception as e:
        rr = normalized.get("raw") if isinstance(normalized.get("raw"), dict) else raw_record
        extract_common.add_error(
            rr,
            error_code="rules_error",
            stage="rules.privacy",
            message_short=utils.short_exc_message(e),
        )

    try:
        rules_timeline.apply_timeline_checks(normalized)
    except Exception as e:
        rr = normalized.get("raw") if isinstance(normalized.get("raw"), dict) else raw_record
        extract_common.add_error(
            rr,
            error_code="rules_error",
            stage="rules.timeline",
            message_short=utils.short_exc_message(e),
        )

    return normalized


def scan_iter(
    root: str,
    *,
    recursive: bool,
    include_exts: Optional[set[str]] = None,
    exclude_patterns: Optional[Iterable[str]] = None,
    hash_algo: str = "none",
    redact: bool = False,
    threads: int = 1,
    cancel: Optional[utils.CancelToken] = None,
) -> Iterator[dict]:
    """레코드를 스트리밍으로 생성합니다.

    - threads>1이면 ThreadPoolExecutor로 병렬 처리
    - cancel이 set되면 가능한 빨리 중단(완료된 레코드까지만 yield)
    """

    root_path = Path(root)
    base_for_rel = root_path if root_path.is_dir() else root_path.parent

    enum = enumerate_files(
        root,
        recursive=recursive,
        include_exts=include_exts,
        exclude_patterns=exclude_patterns,
    )

    # enumerate 단계의 에러는 레코드로 만들 수 없으므로 호출자가 enum.error_messages를 별도 처리합니다.
    files = list(enum.files)
    if not files:
        return

    t = int(threads or 1)
    if t < 1:
        t = 1

    if t == 1:
        for p in files:
            if cancel is not None and cancel.is_cancelled():
                break
            yield _process_one(p, base_for_rel=base_for_rel, hash_algo=hash_algo, redact=bool(redact))
        return

    max_workers = t
    max_in_flight = max_workers * 4
    pending: set[concurrent.futures.Future] = set()

    def submit(ex: concurrent.futures.Executor, path_text: str) -> None:
        pending.add(ex.submit(_process_one, path_text, base_for_rel=base_for_rel, hash_algo=hash_algo, redact=bool(redact)))

    with concurrent.futures.ThreadPoolExecutor(max_workers=max_workers) as ex:
        it = iter(files)

        # initial fill
        while len(pending) < max_in_flight:
            if cancel is not None and cancel.is_cancelled():
                break
            try:
                p = next(it)
            except StopIteration:
                break
            submit(ex, p)

        while pending:
            if cancel is not None and cancel.is_cancelled():
                break

            done, pending = concurrent.futures.wait(
                pending, timeout=0.1, return_when=concurrent.futures.FIRST_COMPLETED
            )

            for fut in done:
                try:
                    yield fut.result()
                except Exception as e:
                    raw = {"path": "", "name": "", "ext": "", "errors": []}
                    extract_common.add_error(
                        raw,
                        error_code="internal_error",
                        stage="engine",
                        message_short=utils.short_exc_message(e),
                    )
                    yield normalize.normalize_record(raw)

            # refill
            while len(pending) < max_in_flight:
                if cancel is not None and cancel.is_cancelled():
                    break
                try:
                    p = next(it)
                except StopIteration:
                    break
                submit(ex, p)

        if cancel is not None and cancel.is_cancelled():
            for fut in list(pending):
                fut.cancel()


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
    redact: bool = False,
    threads: int = 1,
    cancel: Optional[utils.CancelToken] = None,
) -> ScanResult:
    enum = enumerate_files(
        root,
        recursive=recursive,
        include_exts=include_exts,
        exclude_patterns=exclude_patterns,
    )

    records = list(
        scan_iter(
            root,
            recursive=recursive,
            include_exts=include_exts,
            exclude_patterns=exclude_patterns,
            hash_algo=hash_algo,
            redact=redact,
            threads=int(threads or 1),
            cancel=cancel,
        )
    )

    # enumerate 중 발생한 오류(파일 접근/열거 등)만 별도 에러로 유지
    errors: list[str] = list(enum.error_messages)

    return ScanResult(records=records, found=enum.found, excluded=enum.excluded, errors=len(errors), error_messages=errors)
