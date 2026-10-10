from __future__ import annotations

import json
import os
import sqlite3
import threading
import uuid
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable

from .jsonio import dumps_json
from .models import require_valid_records


APPLICATION_ID = 0x4D584354
SCHEMA_REVISION = 1


def _utc_now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


@dataclass(frozen=True)
class CaseInfo:
    case_id: str
    title: str
    notes: str
    created_at: str
    updated_at: str


@dataclass(frozen=True)
class ScanSummary:
    scan_id: str
    source_path: str
    source_kind: str
    origin: str
    imported_from: str | None
    status: str
    started_at: str
    completed_at: str | None
    record_count: int
    warning_count: int
    error_count: int


class CaseWorkspace:
    def __init__(self, path: Path, connection: sqlite3.Connection) -> None:
        self.path = path
        self._connection = connection
        self._lock = threading.RLock()
        self._closed = False

    @classmethod
    def create(
        cls,
        path: str | os.PathLike[str],
        *,
        title: str | None = None,
        case_id: str | None = None,
    ) -> CaseWorkspace:
        workspace_path = Path(path).expanduser().absolute()
        if workspace_path.exists():
            raise FileExistsError(f"case workspace already exists: {workspace_path}")
        if workspace_path.is_symlink():
            raise ValueError("case workspace cannot be a symbolic link")
        workspace_path.parent.mkdir(parents=True, exist_ok=True)
        connection = cls._connect(workspace_path)
        workspace = cls(workspace_path, connection)
        now = _utc_now()
        try:
            with workspace._lock, connection:
                connection.executescript(
                    """
                    CREATE TABLE case_info (
                        singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                        case_id TEXT NOT NULL UNIQUE,
                        title TEXT NOT NULL,
                        notes TEXT NOT NULL DEFAULT '',
                        created_at TEXT NOT NULL,
                        updated_at TEXT NOT NULL
                    );

                    CREATE TABLE sources (
                        source_id INTEGER PRIMARY KEY,
                        path TEXT NOT NULL COLLATE NOCASE UNIQUE,
                        kind TEXT NOT NULL,
                        added_at TEXT NOT NULL
                    );

                    CREATE TABLE scan_runs (
                        scan_id TEXT PRIMARY KEY,
                        source_id INTEGER NOT NULL REFERENCES sources(source_id),
                        origin TEXT NOT NULL,
                        imported_from TEXT,
                        status TEXT NOT NULL CHECK (
                            status IN ('running', 'completed', 'cancelled', 'failed')
                        ),
                        started_at TEXT NOT NULL,
                        completed_at TEXT,
                        record_count INTEGER NOT NULL DEFAULT 0 CHECK (record_count >= 0),
                        warning_count INTEGER NOT NULL DEFAULT 0 CHECK (warning_count >= 0),
                        error_count INTEGER NOT NULL DEFAULT 0 CHECK (error_count >= 0)
                    );

                    CREATE INDEX scan_runs_started_at_idx
                        ON scan_runs(started_at DESC);

                    CREATE TABLE records (
                        scan_id TEXT NOT NULL REFERENCES scan_runs(scan_id) ON DELETE CASCADE,
                        ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
                        path TEXT NOT NULL COLLATE NOCASE,
                        mime TEXT NOT NULL,
                        size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
                        sha256 TEXT NOT NULL,
                        metadata_json TEXT NOT NULL,
                        warnings_json TEXT NOT NULL,
                        errors_json TEXT NOT NULL,
                        PRIMARY KEY (scan_id, path),
                        UNIQUE (scan_id, ordinal)
                    );

                    CREATE INDEX records_scan_ordinal_idx
                        ON records(scan_id, ordinal);
                    """
                )
                connection.execute(
                    """
                    INSERT INTO case_info (
                        singleton, case_id, title, notes, created_at, updated_at
                    ) VALUES (1, ?, ?, '', ?, ?)
                    """,
                    (
                        case_id or str(uuid.uuid4()),
                        (title or workspace_path.stem).strip() or workspace_path.stem,
                        now,
                        now,
                    ),
                )
                connection.execute(f"PRAGMA application_id = {APPLICATION_ID}")
                connection.execute(f"PRAGMA user_version = {SCHEMA_REVISION}")
            try:
                workspace_path.chmod(0o600)
            except OSError:
                pass
            return workspace
        except Exception:
            workspace.close()
            workspace_path.unlink(missing_ok=True)
            raise

    @classmethod
    def open(cls, path: str | os.PathLike[str]) -> CaseWorkspace:
        workspace_path = Path(path).expanduser().absolute()
        if workspace_path.is_symlink():
            raise ValueError("case workspace cannot be a symbolic link")
        if not workspace_path.is_file():
            raise FileNotFoundError(f"case workspace not found: {workspace_path}")
        connection = cls._connect(workspace_path)
        workspace = cls(workspace_path, connection)
        try:
            workspace._validate_database()
        except Exception:
            workspace.close()
            raise
        return workspace

    @staticmethod
    def _connect(path: Path) -> sqlite3.Connection:
        connection = None
        try:
            connection = sqlite3.connect(
                path,
                timeout=15,
                check_same_thread=False,
            )
            connection.row_factory = sqlite3.Row
            connection.execute("PRAGMA foreign_keys = ON")
            connection.execute("PRAGMA journal_mode = DELETE")
            connection.execute("PRAGMA synchronous = FULL")
            connection.execute("PRAGMA trusted_schema = OFF")
            connection.execute("PRAGMA busy_timeout = 15000")
            return connection
        except sqlite3.Error as exc:
            if connection is not None:
                connection.close()
            raise ValueError(f"cannot open case workspace: {exc}") from exc

    def _validate_database(self) -> None:
        try:
            application_id = int(self._connection.execute("PRAGMA application_id").fetchone()[0])
            revision = int(self._connection.execute("PRAGMA user_version").fetchone()[0])
            if application_id != APPLICATION_ID:
                raise ValueError("file is not a MetaXtract case workspace")
            if revision != SCHEMA_REVISION:
                raise ValueError("unsupported case workspace format")
            required_tables = {"case_info", "sources", "scan_runs", "records"}
            rows = self._connection.execute(
                "SELECT name FROM sqlite_schema WHERE type = 'table'"
            ).fetchall()
            if not required_tables.issubset({str(row[0]) for row in rows}):
                raise ValueError("case workspace is missing required data")
            self.info()
        except sqlite3.Error as exc:
            raise ValueError(f"invalid case workspace: {exc}") from exc

    def __enter__(self) -> CaseWorkspace:
        self._ensure_open()
        return self

    def __exit__(self, _exc_type: object, _exc: object, _traceback: object) -> None:
        self.close()

    def _ensure_open(self) -> None:
        if self._closed:
            raise RuntimeError("case workspace is closed")

    def close(self) -> None:
        with self._lock:
            if self._closed:
                return
            self._connection.close()
            self._closed = True

    def info(self) -> CaseInfo:
        with self._lock:
            self._ensure_open()
            row = self._connection.execute(
                """
                SELECT case_id, title, notes, created_at, updated_at
                FROM case_info WHERE singleton = 1
                """
            ).fetchone()
        if row is None:
            raise ValueError("case workspace is missing case information")
        return CaseInfo(
            case_id=str(row["case_id"]),
            title=str(row["title"]),
            notes=str(row["notes"]),
            created_at=str(row["created_at"]),
            updated_at=str(row["updated_at"]),
        )

    def set_case_details(self, *, title: str | None = None, notes: str | None = None) -> None:
        current = self.info()
        next_title = current.title if title is None else title.strip()
        next_notes = current.notes if notes is None else notes
        if not next_title:
            raise ValueError("case title cannot be empty")
        with self._lock, self._connection:
            self._ensure_open()
            self._connection.execute(
                """
                UPDATE case_info
                SET title = ?, notes = ?, updated_at = ?
                WHERE singleton = 1
                """,
                (next_title, next_notes, _utc_now()),
            )

    def save_scan(
        self,
        records: Iterable[Any],
        *,
        source_path: str | os.PathLike[str],
        source_kind: str,
        origin: str = "scan",
        imported_from: str | os.PathLike[str] | None = None,
        started_at: str | None = None,
        completed_at: str | None = None,
    ) -> str:
        valid_records = require_valid_records(records)
        source = str(Path(source_path).expanduser().absolute())
        if not source_kind.strip():
            raise ValueError("source kind cannot be empty")
        scan_id = str(uuid.uuid4())
        start = started_at or _utc_now()
        completed = completed_at or _utc_now()
        warning_count = sum(len(row["warnings"]) for row in valid_records)
        error_count = sum(len(row["errors"]) for row in valid_records)
        imported = (
            str(Path(imported_from).expanduser().absolute())
            if imported_from is not None
            else None
        )

        with self._lock, self._connection:
            self._ensure_open()
            self._connection.execute(
                """
                INSERT INTO sources(path, kind, added_at)
                VALUES (?, ?, ?)
                ON CONFLICT(path) DO UPDATE SET kind = excluded.kind
                """,
                (source, source_kind.strip(), start),
            )
            source_id = int(
                self._connection.execute(
                    "SELECT source_id FROM sources WHERE path = ? COLLATE NOCASE",
                    (source,),
                ).fetchone()[0]
            )
            self._connection.execute(
                """
                INSERT INTO scan_runs (
                    scan_id, source_id, origin, imported_from, status,
                    started_at, completed_at, record_count, warning_count, error_count
                ) VALUES (?, ?, ?, ?, 'completed', ?, ?, ?, ?, ?)
                """,
                (
                    scan_id,
                    source_id,
                    origin,
                    imported,
                    start,
                    completed,
                    len(valid_records),
                    warning_count,
                    error_count,
                ),
            )
            self._connection.executemany(
                """
                INSERT INTO records (
                    scan_id, ordinal, path, mime, size_bytes, sha256,
                    metadata_json, warnings_json, errors_json
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                [
                    (
                        scan_id,
                        ordinal,
                        row["path"],
                        row["mime"],
                        row["size_bytes"],
                        row["sha256"],
                        dumps_json(row["metadata"]),
                        dumps_json(row["warnings"]),
                        dumps_json(row["errors"]),
                    )
                    for ordinal, row in enumerate(valid_records)
                ],
            )
            self._touch(completed)
        return scan_id

    def list_scans(self) -> list[ScanSummary]:
        with self._lock:
            self._ensure_open()
            rows = self._connection.execute(
                """
                SELECT
                    r.scan_id, s.path AS source_path, s.kind AS source_kind,
                    r.origin, r.imported_from, r.status, r.started_at,
                    r.completed_at, r.record_count, r.warning_count, r.error_count
                FROM scan_runs AS r
                JOIN sources AS s ON s.source_id = r.source_id
                ORDER BY r.started_at DESC, r.rowid DESC
                """
            ).fetchall()
        return [
            ScanSummary(
                scan_id=str(row["scan_id"]),
                source_path=str(row["source_path"]),
                source_kind=str(row["source_kind"]),
                origin=str(row["origin"]),
                imported_from=(
                    str(row["imported_from"])
                    if row["imported_from"] is not None
                    else None
                ),
                status=str(row["status"]),
                started_at=str(row["started_at"]),
                completed_at=(
                    str(row["completed_at"])
                    if row["completed_at"] is not None
                    else None
                ),
                record_count=int(row["record_count"]),
                warning_count=int(row["warning_count"]),
                error_count=int(row["error_count"]),
            )
            for row in rows
        ]

    def latest_scan_id(self) -> str | None:
        scans = self.list_scans()
        return scans[0].scan_id if scans else None

    def load_records(self, scan_id: str | None = None) -> list[dict[str, Any]]:
        selected_scan = scan_id or self.latest_scan_id()
        if selected_scan is None:
            return []
        with self._lock:
            self._ensure_open()
            rows = self._connection.execute(
                """
                SELECT path, mime, size_bytes, sha256,
                       metadata_json, warnings_json, errors_json
                FROM records
                WHERE scan_id = ?
                ORDER BY ordinal
                """,
                (selected_scan,),
            ).fetchall()
        return [
            {
                "path": str(row["path"]),
                "mime": str(row["mime"]),
                "size_bytes": int(row["size_bytes"]),
                "sha256": str(row["sha256"]),
                "metadata": json.loads(row["metadata_json"]),
                "warnings": json.loads(row["warnings_json"]),
                "errors": json.loads(row["errors_json"]),
            }
            for row in rows
        ]

    def _touch(self, timestamp: str | None = None) -> None:
        self._connection.execute(
            "UPDATE case_info SET updated_at = ? WHERE singleton = 1",
            (timestamp or _utc_now(),),
        )
