import json
import shutil
from datetime import datetime, timezone
from fractions import Fraction
from pathlib import Path
from types import SimpleNamespace

import pytest
from PIL import Image

import metaxtract.extractors.video as video_module
from metaxtract.core.scanner import scan_file
from metaxtract.extractors.image import _extract_gps, extract_image
from metaxtract.extractors.normalize import normalize_metadata
from metaxtract.extractors.video import extract_video


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
    assert pdf.metadata["pdf_encrypted"] is False
    assert pdf.metadata["pdf_creation_date"].endswith("+00:00")
    assert pdf.metadata["pdf_modification_date"].endswith("+00:00")
    assert docx.path == "sample.docx"
    assert docx.metadata["docx_author"] == "MetaXtract"
    assert docx.metadata["docx_title"] == "MetaXtract DOCX"
    assert docx.metadata["docx_created"].endswith("+00:00")
    assert docx.metadata["docx_modified"].endswith("+00:00")
    assert docx.metadata["docx_revision"] == 1


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
        assert record.metadata["video_stream_count"] == 1
        assert record.metadata["video_frame_rate"] == pytest.approx(10.0)
        assert record.metadata["video_stream_types"] == {"video": 1}
        assert isinstance(record.metadata["video_tags"], dict)
    else:
        assert "ffprobe_not_found" in record.warnings


def test_image_extracts_orientation_software_and_altitude(tmp_path) -> None:
    image_path = tmp_path / "oriented.jpg"
    image = Image.new("RGB", (8, 4), "white")
    exif = Image.Exif()
    exif[274] = 6
    exif[305] = "MetaXtract Tests"
    image.save(image_path, exif=exif)

    metadata, warnings = extract_image(image_path)
    gps = _extract_gps(
        {
            1: "S",
            2: (37, 30, 0),
            3: "W",
            4: (126, 30, 0),
            5: b"\x01",
            6: (125, 1),
        }
    )

    assert warnings == []
    assert metadata["exif_orientation"] == 6
    assert metadata["exif_orientation_label"] == "rotated_90"
    assert metadata["exif_software"] == "MetaXtract Tests"
    assert gps["gps_latitude"] == pytest.approx(-37.5)
    assert gps["gps_longitude"] == pytest.approx(-126.5)
    assert gps["gps_altitude_m"] == pytest.approx(-125.0)


def test_video_extracts_audio_streams_tags_and_frame_rate(monkeypatch) -> None:
    probe_data = {
        "format": {
            "duration": "2.5",
            "format_name": "mov,mp4",
            "bit_rate": "128000",
            "tags": {"creation_time": "2026-01-01T00:00:00Z"},
        },
        "streams": [
            {
                "index": 0,
                "codec_type": "video",
                "codec_name": "h264",
                "width": 1920,
                "height": 1080,
                "avg_frame_rate": "30000/1001",
            },
            {
                "index": 1,
                "codec_type": "audio",
                "codec_name": "aac",
                "sample_rate": "48000",
                "channels": 2,
                "channel_layout": "stereo",
                "tags": {"language": "kor"},
            },
        ],
    }
    monkeypatch.setattr(video_module, "_ffprobe_available", lambda: True)
    monkeypatch.setattr(
        video_module.subprocess,
        "run",
        lambda *_args, **_kwargs: SimpleNamespace(
            returncode=0,
            stdout=json.dumps(probe_data),
        ),
    )

    metadata, warnings = extract_video("sample.mp4")

    assert warnings == []
    assert metadata["video_stream_types"] == {"audio": 1, "video": 1}
    assert metadata["video_frame_rate"] == pytest.approx(30000 / 1001)
    assert metadata["audio_codec"] == "aac"
    assert metadata["audio_sample_rate"] == 48000
    assert metadata["audio_channels"] == 2
    assert metadata["audio_channel_layout"] == "stereo"
    assert metadata["video_streams"][1]["tags"] == {"language": "kor"}


def test_metadata_normalization_is_json_safe() -> None:
    metadata = normalize_metadata(
        {
            "timestamp": datetime(2026, 1, 2, 3, 4, tzinfo=timezone.utc),
            "ratio": Fraction(1, 4),
            "bytes": b"camera\x00",
            "non_finite": float("inf"),
            "nested": {1: ("value", Fraction(1, 2))},
        }
    )

    assert metadata == {
        "timestamp": "2026-01-02T03:04:00+00:00",
        "ratio": 0.25,
        "bytes": "camera",
        "non_finite": None,
        "nested": {"1": ["value", 0.5]},
    }
    json.dumps(metadata, allow_nan=False)
