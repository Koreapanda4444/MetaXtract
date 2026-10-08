from pathlib import Path

import pytest

from metaxtract.extract_image import extract_image


def test_extracts_gps_coordinates() -> None:
    fixture = Path(__file__).parent / "fixtures" / "sample_gps.jpg"
    metadata, warnings = extract_image(fixture)

    assert warnings == []
    assert metadata["gps_latitude"] == pytest.approx(37.5666666667)
    assert metadata["gps_longitude"] == pytest.approx(126.9666666667)
