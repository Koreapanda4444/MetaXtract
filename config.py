from __future__ import annotations

from dataclasses import dataclass

@dataclass(frozen=True)
class Settings:
    threads: int = 4
    hash_algo: str = "sha256"
    save_raw: bool = False
    redact: bool = False
