
import json
import zipfile

import pytest

from bundle_export import export_case_bundle


def test_export_case_bundle(tmp_path):
    # 샘플 scan.jsonl 생성
    scan_path = tmp_path / "scan.jsonl"
    records = [
        {"path": "a.txt", "sha256": "dummyhash", "mime": "text/plain", "size_bytes": 1},
        {"path": "b.txt", "sha256": "dummyhash2", "mime": "text/plain", "size_bytes": 2},
    ]
    with open(scan_path, "w", encoding="utf-8") as f:
        import json
        for r in records:
            f.write(json.dumps(r) + "\n")

    out_zip = tmp_path / "case.zip"
    export_case_bundle(scan_path, out_zip)

    with zipfile.ZipFile(out_zip, "r") as zf:
        names = set(zf.namelist())
        assert "scan.jsonl" in names
        assert "manifest.json" in names
        assert "hashes.txt" in names
        assert "reports/report.json" in names


def test_redacted_bundle_removes_sensitive_metadata(tmp_path):
    scan_path = tmp_path / "scan.jsonl"
    record = {
        "path": "photo.jpg",
        "sha256": "dummyhash",
        "mime": "image/jpeg",
        "size_bytes": 1,
        "metadata": {
            "gps_latitude": 37.5,
            "gps_longitude": 127.0,
            "exif_datetime_original": "2026:01:02 03:04:05",
            "docx_author": "Private Person",
            "width": 100,
        },
    }
    scan_path.write_text(json.dumps(record) + "\n", encoding="utf-8")
    out_zip = tmp_path / "redacted.zip"

    export_case_bundle(scan_path, out_zip, redact=True)

    with zipfile.ZipFile(out_zip, "r") as zf:
        bundled_scan = json.loads(zf.read("scan.jsonl"))
        manifest = json.loads(zf.read("manifest.json"))
        report = json.loads(zf.read("reports/report.json"))

    assert bundled_scan["metadata"] == {"width": 100}
    assert manifest["redacted"] is True
    assert report["findings"]["gps_files"] == []
    assert report["findings"]["authors"] == {}


def test_redaction_rejects_raw_file_inclusion(tmp_path):
    scan_path = tmp_path / "scan.jsonl"
    scan_path.write_text("", encoding="utf-8")

    with pytest.raises(ValueError, match="cannot be combined"):
        export_case_bundle(
            scan_path,
            tmp_path / "case.zip",
            redact=True,
            include_files=True,
        )


def test_file_inclusion_uses_scan_directory_by_default(tmp_path):
    source = tmp_path / "evidence.txt"
    source.write_text("evidence", encoding="utf-8")
    scan_path = tmp_path / "scan.jsonl"
    scan_path.write_text(
        json.dumps({"path": "evidence.txt", "sha256": "dummyhash"}) + "\n",
        encoding="utf-8",
    )
    out_zip = tmp_path / "case.zip"

    export_case_bundle(scan_path, out_zip, include_files=True)

    with zipfile.ZipFile(out_zip, "r") as zf:
        assert zf.read("files/evidence.txt") == b"evidence"


@pytest.mark.parametrize("unsafe_path", ["../secret.txt", "/etc/passwd", "C:\\secret.txt"])
def test_export_rejects_unsafe_paths(tmp_path, unsafe_path):
    scan_path = tmp_path / "scan.jsonl"
    scan_path.write_text(
        json.dumps({"path": unsafe_path, "sha256": "dummyhash"}) + "\n",
        encoding="utf-8",
    )

    with pytest.raises(ValueError):
        export_case_bundle(scan_path, tmp_path / "case.zip")
