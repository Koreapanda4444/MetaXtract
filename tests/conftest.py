from __future__ import annotations

from pathlib import Path

import pytest

from tests.gen_fixtures import ensure_fixtures
from tests.gen_golden import ensure_golden


@pytest.fixture(scope="session", autouse=True)
def _prepare_test_assets() -> None:
    fixtures_dir = Path(__file__).parent / "fixtures"
    golden_dir = Path(__file__).parent / "golden"
    ensure_fixtures(fixtures_dir)
    ensure_golden(fixtures_dir, golden_dir)
