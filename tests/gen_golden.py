from __future__ import annotations

from pathlib import Path
from typing import Dict

from engine import scan_file
from utils import dumps_json


def _golden_name_for_fixture(fixture_path: Path) -> str:
    return fixture_path.name + ".jsonl"


def ensure_golden(fixtures_dir: Path, golden_dir: Path) -> Dict[str, bool]:
    golden_dir.mkdir(parents=True, exist_ok=True)

    created: Dict[str, bool] = {}
    for fp in sorted(fixtures_dir.glob("*")):
        if not fp.is_file():
            continue
        gp = golden_dir / _golden_name_for_fixture(fp)
        if gp.exists():
            created[gp.name] = False
            continue

        rec = scan_file(fp, base=fixtures_dir)
        gp.write_text(dumps_json(rec) + "\n", encoding="utf-8")
        created[gp.name] = True

    return created
