from __future__ import annotations

import json
from pathlib import Path

from engine import scan_file
from utils import dumps_json


_VOLATILE_KEYS = {"mtime", "atime", "ctime"}


def _normalize(obj):
    if isinstance(obj, dict):
        out = {}
        for k, v in obj.items():
            if k in _VOLATILE_KEYS and isinstance(v, int):
                out[k] = 0
            else:
                out[k] = _normalize(v)
        return out
    if isinstance(obj, list):
        return [_normalize(x) for x in obj]
    return obj


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

        golden_line = next(line for line in golden.read_text(encoding="utf-8").splitlines() if line.strip())

        expected_obj = json.loads(golden_line)
        actual_obj = json.loads(dumps_json(record))

        assert _normalize(actual_obj) == _normalize(expected_obj)
