from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Settings:
    max_files: int = 5000
    include_hidden: bool = False
