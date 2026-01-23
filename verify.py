from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import List

from utils import read_jsonl, sha256_file


@dataclass(frozen=True)
class VerifyIssue:
    path: str
    issue: str


def verify_scan(scan_jsonl_path: str, base_dir: str) -> List[VerifyIssue]:
    base = Path(base_dir)
    rows = read_jsonl(scan_jsonl_path)
    issues: List[VerifyIssue] = []

    for r in rows:
        rel = r.get("path", "")
        expected = r.get("sha256", "")
        fp = base / rel
        if not fp.exists():
            issues.append(VerifyIssue(path=rel, issue="missing"))
            continue
        actual = sha256_file(fp)
        if expected and expected != actual:
            issues.append(VerifyIssue(path=rel, issue="hash_mismatch"))

    return issues
