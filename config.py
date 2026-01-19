from __future__ import annotations

from dataclasses import dataclass

@dataclass(frozen=True)
class Settings:
    threads: int = 4
    hash_algo: str = "sha256"
    save_raw: bool = False
    redact: bool = False
    scan_recursive: bool = False
    scan_include: str | None = None
    scan_exclude: tuple[str, ...] = ()
    scan_hash: str = "none"

    sanitize_mode: str = "redact"
