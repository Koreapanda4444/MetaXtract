"""tests/golden/ 재생성 스크립트.

사용법:
    python scripts/regen_golden.py
또는:
    make regen-golden
"""
from __future__ import annotations

import sys
from pathlib import Path

# 프로젝트 루트를 sys.path에 추가
ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "tests"))

from gen_golden import ensure_golden  # noqa: E402

if __name__ == "__main__":
    fixtures_dir = ROOT / "tests" / "fixtures"
    golden_dir = ROOT / "tests" / "golden"
    result = ensure_golden(fixtures_dir, golden_dir)
    for name, created in result.items():
        status = "created" if created else "exists"
        print(f"  [{status}] {name}")
    print("Done.")
