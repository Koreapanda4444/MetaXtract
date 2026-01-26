import json
import hashlib
from pathlib import Path
from threading import RLock


class CacheStore:
    def __init__(self, cache_dir: str = ".metaxtract_cache"):
        self.cache_dir = Path(cache_dir)
        self.cache_dir.mkdir(parents=True, exist_ok=True)
        self.index_path = self.cache_dir / "cache_index.jsonl"
        self.lock = RLock()

    def _file_key(self, path: str, mode: str = "sha256"):
        p = Path(path)
        if mode == "sha256":
            h = hashlib.sha256()
            with open(p, "rb") as f:
                while True:
                    chunk = f.read(8192)
                    if not chunk:
                        break
                    h.update(chunk)
            return f"sha256:{h.hexdigest()}"
        elif mode == "mtime":
            stat = p.stat()
            return f"mtime:{int(stat.st_mtime)}:{stat.st_size}"
        else:
            raise ValueError(f"Unknown cache mode: {mode}")

    def get(self, path: str, mode: str = "sha256"):
        key = self._file_key(path, mode)
        with self.lock:
            if not self.index_path.exists():
                return None
            with open(self.index_path, "r", encoding="utf-8") as f:
                for line in f:
                    rec = json.loads(line)
                    if rec.get("key") == key:
                        return rec.get("result")
        return None

    def set(self, path: str, result, mode: str = "sha256"):
        key = self._file_key(path, mode)
        rec = {"key": key, "result": result}
        with self.lock:
            with open(self.index_path, "a", encoding="utf-8") as f:
                f.write(json.dumps(rec, ensure_ascii=False) + "\n")

    def purge(self):
        with self.lock:
            if self.index_path.exists():
                self.index_path.unlink()

    def stats(self):
        with self.lock:
            if not self.index_path.exists():
                return {"entries": 0, "size": 0}
            size = self.index_path.stat().st_size
            with open(self.index_path, "r", encoding="utf-8") as f:
                entries = sum(1 for _ in f)
            return {"entries": entries, "size": size}
