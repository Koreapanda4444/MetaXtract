from __future__ import annotations

from pathlib import Path

from engine import scan_file
from utils import dumps_json


def test_scan_matches_golden() -> None:
    tests_dir = Path(__file__).parent
    fixtures_dir = tests_dir / "fixtures"
    golden_dir = tests_dir / "golden"

    for fixture in sorted(fixtures_dir.glob("*")):
        if not fixture.is_file():
            continue

        golden = golden_dir / (fixture.name + ".jsonl")
        assert golden.exists(), f"Missing golden file: {golden}"

        record = scan_file(fixture, base=fixtures_dir)
        expected = golden.read_text(encoding="utf-8")
        actual = dumps_json(record) + "\n"
        assert actual == expected
