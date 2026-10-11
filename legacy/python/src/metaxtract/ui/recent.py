from __future__ import annotations

import json
import os
import stat
import sys
import tempfile
from pathlib import Path


MAX_RECENT_CASES = 10


def default_recent_path() -> Path:
    if os.name == "nt":
        base = os.environ.get("LOCALAPPDATA") or os.environ.get("APPDATA")
        root = Path(base) if base else Path.home() / "AppData" / "Local"
        return root / "MetaXtract" / "recent-cases.json"
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "MetaXtract" / "recent-cases.json"
    base = os.environ.get("XDG_CONFIG_HOME")
    root = Path(base) if base else Path.home() / ".config"
    return root / "metaxtract" / "recent-cases.json"


class RecentCaseStore:
    def __init__(self, path: str | os.PathLike[str] | None = None) -> None:
        self.path = Path(path) if path is not None else default_recent_path()

    def load(self) -> list[str]:
        try:
            data = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, UnicodeError, json.JSONDecodeError):
            return []
        if not isinstance(data, list):
            return []
        recent = []
        seen = set()
        for value in data:
            if not isinstance(value, str) or not value.strip():
                continue
            normalized = str(Path(value).expanduser().absolute())
            key = normalized.casefold()
            if key in seen:
                continue
            seen.add(key)
            recent.append(normalized)
            if len(recent) == MAX_RECENT_CASES:
                break
        return recent

    def add(self, path: str | os.PathLike[str]) -> list[str]:
        normalized = str(Path(path).expanduser().absolute())
        recent = [
            item for item in self.load() if item.casefold() != normalized.casefold()
        ]
        recent.insert(0, normalized)
        self._save(recent[:MAX_RECENT_CASES])
        return recent[:MAX_RECENT_CASES]

    def remove(self, path: str | os.PathLike[str]) -> list[str]:
        normalized = str(Path(path).expanduser().absolute()).casefold()
        recent = [item for item in self.load() if item.casefold() != normalized]
        self._save(recent)
        return recent

    def _save(self, recent: list[str]) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        mode = stat.S_IMODE(self.path.stat().st_mode) if self.path.exists() else 0o600
        handle = tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            newline="\n",
            prefix=f".{self.path.name}.",
            suffix=".tmp",
            dir=self.path.parent,
            delete=False,
        )
        temp_path = Path(handle.name)
        try:
            with handle as stream:
                json.dump(recent, stream, ensure_ascii=False, indent=2)
                stream.write("\n")
                stream.flush()
                os.fsync(stream.fileno())
            temp_path.chmod(mode)
            temp_path.replace(self.path)
        except Exception:
            handle.close()
            temp_path.unlink(missing_ok=True)
            raise
