from __future__ import annotations

import queue
import threading
from typing import Any

from metaxtract.core.cache import CacheStore
from metaxtract.core.scanner import ScanCancelled, scan_path
from metaxtract.core.workspace import CaseWorkspace


EventQueue = queue.Queue[tuple[str, Any]]


def run_scan(
    events: EventQueue,
    target: str,
    use_cache: bool,
    include_hidden: bool,
    max_files: int,
    cancel_event: threading.Event,
    workspace: CaseWorkspace | None = None,
    scan_id: str | None = None,
) -> None:
    def report_progress(completed: int, total: int, path: str | None) -> None:
        events.put(("progress", (completed, total, path)))

    last_byte_report: dict[str, int] = {}

    def report_bytes(
        file_index: int,
        total_files: int,
        path: str,
        processed: int,
        total_bytes: int,
    ) -> None:
        previous = last_byte_report.get(path, -8 * 1024 * 1024)
        if processed not in {0, total_bytes} and processed - previous < 8 * 1024 * 1024:
            return
        last_byte_report[path] = processed
        events.put(("bytes", (file_index, total_files, path, processed, total_bytes)))

    def persist_record(record: Any, completed: int, _total: int) -> None:
        if workspace is not None and scan_id is not None:
            workspace.append_record(scan_id, record, ordinal=completed - 1)

    def finish_persistent_scan(status: str) -> None:
        if workspace is not None and scan_id is not None:
            workspace.finish_scan(scan_id, status=status)

    try:
        cache = CacheStore(".metaxtract_cache") if use_cache else None
        records = scan_path(
            target,
            cache=cache,
            cache_enabled=use_cache,
            max_files=max_files,
            include_hidden=include_hidden,
            progress_callback=report_progress,
            byte_progress_callback=report_bytes,
            record_callback=persist_record,
            cancel_check=cancel_event.is_set,
        )
    except ScanCancelled:
        try:
            finish_persistent_scan("cancelled")
        except Exception as exc:
            events.put(("error", (type(exc).__name__, str(exc))))
        else:
            events.put(("cancelled", None))
    except Exception as exc:
        try:
            finish_persistent_scan("failed")
        except Exception:
            pass
        events.put(("error", (type(exc).__name__, str(exc))))
    else:
        try:
            finish_persistent_scan("completed")
        except Exception as exc:
            events.put(("error", (type(exc).__name__, str(exc))))
        else:
            events.put(("complete", records))


def run_verification(
    events: EventQueue,
    bundle: str,
    files_base: str | None,
    public_key: str | None,
) -> None:
    from metaxtract.case.verify import verify_bundle

    try:
        issues = verify_bundle(
            bundle,
            files_base=files_base,
            public_key=public_key,
        )
    except Exception as exc:
        events.put(("verify_error", (type(exc).__name__, str(exc))))
    else:
        events.put(("verify_complete", (bundle, issues)))


def run_import(
    events: EventQueue,
    path: str,
    import_kind: str,
    public_key: str | None = None,
) -> None:
    from metaxtract.case.importer import ImportValidationError, load_bundle, load_jsonl

    try:
        if import_kind == "jsonl":
            payload = load_jsonl(path)
        elif import_kind == "bundle":
            payload = load_bundle(path, public_key=public_key)
        else:
            raise ValueError(f"unsupported import kind: {import_kind}")
    except ImportValidationError as exc:
        events.put(("import_invalid", (path, exc.issues)))
    except Exception as exc:
        events.put(("import_error", (type(exc).__name__, str(exc))))
    else:
        events.put(("import_complete", payload))
