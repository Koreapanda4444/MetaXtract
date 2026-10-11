from __future__ import annotations

import hashlib
import json
from collections.abc import Mapping
from pathlib import Path
from threading import RLock
from typing import Any

from .files import sha256_file


CACHE_VERSION = 2


class CacheStore:
    def __init__(self, cache_dir: str | Path = ".metaxtract_cache"):
        self.cache_dir = Path(cache_dir)
        self.cache_dir.mkdir(parents=True, exist_ok=True)
        self.index_path = self.cache_dir / "cache_index.json"
        self.legacy_index_path = self.cache_dir / "cache_index.jsonl"
        self.lock = RLock()
        self._entries = self._load_entries()
        self._dirty = False

    def _load_entries(self) -> dict[str, dict[str, Any]]:
        if self.index_path.exists():
            try:
                data = json.loads(self.index_path.read_text(encoding="utf-8"))
            except (OSError, UnicodeError, json.JSONDecodeError):
                return {}
            if not isinstance(data, dict) or data.get("version") != CACHE_VERSION:
                return {}
            entries = data.get("entries") if isinstance(data, dict) else None
            if not isinstance(entries, dict):
                return {}
            return {
                key: dict(value)
                for key, value in entries.items()
                if isinstance(key, str) and isinstance(value, dict)
            }

        entries = {}
        if self.legacy_index_path.exists():
            try:
                lines = self.legacy_index_path.read_text(encoding="utf-8").splitlines()
            except (OSError, UnicodeError):
                return {}
            for line in lines:
                try:
                    record = json.loads(line)
                except json.JSONDecodeError:
                    continue
                key = record.get("key") if isinstance(record, dict) else None
                result = record.get("result") if isinstance(record, dict) else None
                if (
                    isinstance(key, str)
                    and key.startswith(f"v{CACHE_VERSION}:")
                    and isinstance(result, dict)
                ):
                    entries[key] = dict(result)
        return entries

    def _save_entries(self) -> None:
        payload = {"version": CACHE_VERSION, "entries": self._entries}
        temp_path = self.index_path.with_suffix(".tmp")
        temp_path.write_text(
            json.dumps(payload, ensure_ascii=False, sort_keys=True),
            encoding="utf-8",
        )
        temp_path.replace(self.index_path)
        if self.legacy_index_path.exists():
            self.legacy_index_path.unlink()

    def _file_key(
        self,
        path: str | Path,
        mode: str = "sha256",
        *,
        content_sha256: str | None = None,
        file_stat: Mapping[str, int] | None = None,
    ) -> str:
        source = Path(path)
        path_hash = hashlib.sha256(
            str(source.resolve()).encode("utf-8", errors="surrogatepass")
        ).hexdigest()
        if mode == "sha256":
            digest = content_sha256 or sha256_file(source)
            return f"v{CACHE_VERSION}:sha256:{path_hash}:{digest}"
        if mode == "mtime":
            if file_stat is None:
                stat = source.stat()
                mtime_ns = stat.st_mtime_ns
                size_bytes = stat.st_size
            else:
                mtime_ns = file_stat["mtime_ns"]
                size_bytes = file_stat["size_bytes"]
            return f"v{CACHE_VERSION}:mtime:{path_hash}:{mtime_ns}:{size_bytes}"
        raise ValueError(f"Unknown cache mode: {mode}")

    def get(
        self,
        path: str | Path,
        mode: str = "sha256",
        *,
        content_sha256: str | None = None,
        file_stat: Mapping[str, int] | None = None,
    ) -> dict[str, Any] | None:
        key = self._file_key(
            path,
            mode,
            content_sha256=content_sha256,
            file_stat=file_stat,
        )
        with self.lock:
            result = self._entries.get(key)
            return dict(result) if isinstance(result, dict) else None

    def set(
        self,
        path: str | Path,
        result: Mapping[str, Any],
        mode: str = "sha256",
        *,
        content_sha256: str | None = None,
        file_stat: Mapping[str, int] | None = None,
        flush: bool = True,
    ) -> None:
        key = self._file_key(
            path,
            mode,
            content_sha256=content_sha256,
            file_stat=file_stat,
        )
        with self.lock:
            self._entries[key] = dict(result)
            self._dirty = True
            if flush:
                self.flush()

    def flush(self) -> None:
        with self.lock:
            if not self._dirty:
                return
            self._save_entries()
            self._dirty = False

    def purge(self) -> None:
        with self.lock:
            self._entries.clear()
            self._dirty = False
            for path in (self.index_path, self.legacy_index_path):
                if path.exists():
                    path.unlink()

    def stats(self) -> dict[str, int]:
        with self.lock:
            size = self.index_path.stat().st_size if self.index_path.exists() else 0
            return {"entries": len(self._entries), "size": size}
