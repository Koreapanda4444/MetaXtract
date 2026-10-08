import shutil
from pathlib import Path

import pytest

from metaxtract.core.scanner import scan_file
from metaxtract.extractors.image import extract_image


FIXTURES = Path(__file__).parent / "fixtures"


def test_extracts_gps_coordinates() -> None:
    metadata, warnings = extract_image(FIXTURES / "sample_gps.jpg")

    assert warnings == []
    assert metadata["gps_latitude"] == pytest.approx(37.5666666667)
    assert metadata["gps_longitude"] == pytest.approx(126.9666666667)


def test_document_fixtures_extract_expected_metadata() -> None:
    pdf = scan_file(FIXTURES / "sample.pdf", base=FIXTURES)
    docx = scan_file(FIXTURES / "sample.docx", base=FIXTURES)

    assert pdf.path == "sample.pdf"
    assert pdf.metadata["pdf_author"] == "MetaXtract"
    assert pdf.metadata["pages"] == 1
    assert docx.path == "sample.docx"
    assert docx.metadata["docx_author"] == "MetaXtract"
    assert docx.metadata["docx_title"] == "MetaXtract DOCX"


def test_image_without_exif_has_only_image_properties() -> None:
    record = scan_file(FIXTURES / "sample_noexif.jpg", base=FIXTURES)

    assert record.metadata["format"] == "JPEG"
    assert record.metadata["width"] == 64
    assert "gps_latitude" not in record.metadata


def test_video_fixture_handles_optional_ffprobe() -> None:
    record = scan_file(FIXTURES / "sample.mp4", base=FIXTURES)

    if shutil.which("ffprobe"):
        assert record.metadata["video_width"] == 64
        assert record.metadata["video_height"] == 64
    else:
        assert "ffprobe_not_found" in record.warnings
