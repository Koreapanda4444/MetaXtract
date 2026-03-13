
from __future__ import annotations

# 케이스 번들 manifest 스키마 (예시)
from dataclasses import dataclass, field
from typing import List, Optional, Any, Dict


@dataclass(frozen=True)
class CaseManifest:
    case_id: Optional[str]
    created_at: str
    notes: str = ""
    record_count: int = 0
    hashes: List[str] = field(default_factory=list)
    redacted: bool = False


@dataclass(frozen=True)
class ScanRecord:
    path: str
    mime: str
    size_bytes: int
    sha256: str
    metadata: Dict[str, Any] = field(default_factory=dict)
    warnings: List[str] = field(default_factory=list)
    errors: List[str] = field(default_factory=list)
