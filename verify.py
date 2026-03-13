
from __future__ import annotations
import zipfile
from pathlib import Path
from dataclasses import dataclass
from utils import sha256_file


def verify_bundle_hashes(bundle_zip_path: str, files_base: str = None):
    """
    번들(zip) 내 hashes.txt와 실제 파일 해시 비교
    """
    issues = []
    with zipfile.ZipFile(bundle_zip_path, "r") as zf:
        hashes_txt = zf.read("hashes.txt").decode("utf-8").splitlines()
        for line in hashes_txt:
            if not line.strip():
                continue
            hashval, rel = line.split("\t", 1)
            if files_base:
                fp = Path(files_base) / rel
                if not fp.exists():
                    issues.append({"path": rel, "issue": "missing"})
                    continue
                actual = sha256_file(fp)
                if hashval != actual:
                    issues.append({"path": rel, "issue": "hash_mismatch"})
    return issues


@dataclass(frozen=True)
class VerifyIssue:
    path: str
    issue: str
