import shutil
from pathlib import Path

from engine import scan_file


FIXTURES = Path(__file__).parent / "fixtures"


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
