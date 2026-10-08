import hashlib
import json
from pathlib import Path
from threading import RLock


class CacheStore:
    def __init__(self, cache_dir: str = ".metaxtract_cache"):
        self.cache_dir = Path(cache_dir)
        self.cache_dir.mkdir(parents=True, exist_ok=True)
        self.index_path = self.cache_dir / "cache_index.json"
        self.legacy_index_path = self.cache_dir / "cache_index.jsonl"
        self.lock = RLock()

    def _load_entries(self):
        if self.index_path.exists():
            try:
                data = json.loads(self.index_path.read_text(encoding="utf-8"))
            except (OSError, UnicodeError, json.JSONDecodeError):
                return {}
            entries = data.get("entries") if isinstance(data, dict) else None
            return entries if isinstance(entries, dict) else {}

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
                if isinstance(key, str) and isinstance(record.get("result"), dict):
                    entries[key] = record["result"]
        return entries

    def _save_entries(self, entries):
        payload = {"version": 1, "entries": entries}
        temp_path = self.index_path.with_suffix(".tmp")
        temp_path.write_text(
            json.dumps(payload, ensure_ascii=False, sort_keys=True),
            encoding="utf-8",
        )
        temp_path.replace(self.index_path)
        if self.legacy_index_path.exists():
            self.legacy_index_path.unlink()

    def _file_key(self, path: str, mode: str = "sha256"):
        p = Path(path)
        path_hash = hashlib.sha256(
            str(p.resolve()).encode("utf-8", errors="surrogatepass")
        ).hexdigest()
        if mode == "sha256":
            h = hashlib.sha256()
            with open(p, "rb") as f:
                while True:
                    chunk = f.read(8192)
                    if not chunk:
                        break
                    h.update(chunk)
            return f"sha256:{path_hash}:{h.hexdigest()}"
        elif mode == "mtime":
            stat = p.stat()
            return f"mtime:{path_hash}:{stat.st_mtime_ns}:{stat.st_size}"
        else:
            raise ValueError(f"Unknown cache mode: {mode}")

    def get(self, path: str, mode: str = "sha256"):
        key = self._file_key(path, mode)
        with self.lock:
            result = self._load_entries().get(key)
            return dict(result) if isinstance(result, dict) else None

    def set(self, path: str, result, mode: str = "sha256"):
        key = self._file_key(path, mode)
        with self.lock:
            entries = self._load_entries()
            entries[key] = result
            self._save_entries(entries)

    def purge(self):
        with self.lock:
            for path in (self.index_path, self.legacy_index_path):
                if path.exists():
                    path.unlink()

    def stats(self):
        with self.lock:
            entries = self._load_entries()
            size = self.index_path.stat().st_size if self.index_path.exists() else 0
            return {"entries": len(entries), "size": size}
