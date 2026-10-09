
import json
import zipfile

import pytest

from metaxtract.case.bundle import export_case_bundle
from metaxtract.case.verify import verify_bundle, verify_scan
from metaxtract.core.files import sha256_file


def _record(
    path,
    *,
    sha256="0" * 64,
    size_bytes=0,
    mime="application/octet-stream",
    metadata=None,
    warnings=None,
    errors=None,
):
    return {
        "path": path,
        "sha256": sha256,
        "size_bytes": size_bytes,
        "mime": mime,
        "metadata": metadata or {},
        "warnings": warnings or [],
        "errors": errors or [],
    }


def _rewrite_zip(source, destination, *, remove=(), replacements=None):
    replacements = replacements or {}
    with zipfile.ZipFile(source, "r") as original:
        with zipfile.ZipFile(destination, "w", compression=zipfile.ZIP_DEFLATED) as output:
            for info in original.infolist():
                if info.filename in remove:
                    continue
                data = replacements.get(info.filename, original.read(info.filename))
                output.writestr(info, data)


def test_export_case_bundle(tmp_path):
    scan_path = tmp_path / "scan.jsonl"
    records = [
        _record("a.txt", sha256="0" * 64, size_bytes=1, mime="text/plain"),
        _record("b.txt", sha256="1" * 64, size_bytes=2, mime="text/plain"),
    ]
    with open(scan_path, "w", encoding="utf-8") as f:
        for r in records:
            f.write(json.dumps(r) + "\n")

    out_zip = tmp_path / "case.zip"
    export_case_bundle(scan_path, out_zip)

    with zipfile.ZipFile(out_zip, "r") as zf:
        names = set(zf.namelist())
        manifest = json.loads(zf.read("manifest.json"))
        assert "scan.jsonl" in names
        assert "manifest.json" in names
        assert "hashes.txt" in names
        assert "reports/report.json" in names
        assert manifest["includes_files"] is False
        assert manifest["original_files"] == []


def test_redacted_bundle_removes_sensitive_metadata(tmp_path):
    scan_path = tmp_path / "scan.jsonl"
    record = _record(
        "photo.jpg",
        size_bytes=1,
        mime="image/jpeg",
        metadata={
            "gps_latitude": 37.5,
            "gps_longitude": 127.0,
            "exif_datetime_original": "2026:01:02 03:04:05",
            "docx_author": "Private Person",
            "width": 100,
        },
    )
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
        json.dumps(
            _record(
                "evidence.txt",
                sha256=sha256_file(source),
                size_bytes=source.stat().st_size,
                mime="text/plain",
            )
        )
        + "\n",
        encoding="utf-8",
    )
    out_zip = tmp_path / "case.zip"

    export_case_bundle(scan_path, out_zip, include_files=True)

    with zipfile.ZipFile(out_zip, "r") as zf:
        assert zf.read("files/evidence.txt") == b"evidence"
        manifest = json.loads(zf.read("manifest.json"))

    assert manifest["includes_files"] is True
    assert manifest["original_files"] == [
        {
            "path": "evidence.txt",
            "sha256": sha256_file(source),
            "size_bytes": source.stat().st_size,
        }
    ]


@pytest.mark.parametrize("unsafe_path", ["../secret.txt", "/etc/passwd", "C:\\secret.txt"])
def test_export_rejects_unsafe_paths(tmp_path, unsafe_path):
    scan_path = tmp_path / "scan.jsonl"
    scan_path.write_text(
        json.dumps(_record(unsafe_path)) + "\n",
        encoding="utf-8",
    )

    with pytest.raises(ValueError):
        export_case_bundle(scan_path, tmp_path / "case.zip")


def test_verify_scan_detects_changes_and_duplicate_paths(tmp_path):
    source = tmp_path / "evidence.txt"
    source.write_text("original", encoding="utf-8")
    record = _record(
        "evidence.txt",
        sha256=sha256_file(source),
        size_bytes=source.stat().st_size,
        mime="text/plain",
    )
    scan_path = tmp_path / "scan.jsonl"
    scan_path.write_text(
        json.dumps(record) + "\n" + json.dumps(record) + "\n",
        encoding="utf-8",
    )
    source.write_text("changed", encoding="utf-8")

    issues = verify_scan(str(scan_path), str(tmp_path))
    issue_names = {issue["issue"] for issue in issues}

    assert "duplicate_path" in issue_names
    assert "hash_mismatch" in issue_names


def test_verify_complete_bundle(tmp_path):
    source = tmp_path / "evidence.txt"
    source.write_text("evidence", encoding="utf-8")
    record = {
        "path": "evidence.txt",
        "sha256": sha256_file(source),
        "size_bytes": source.stat().st_size,
        "mime": "text/plain",
        "metadata": {},
        "warnings": [],
        "errors": [],
    }
    scan_path = tmp_path / "scan.jsonl"
    scan_path.write_text(json.dumps(record) + "\n", encoding="utf-8")
    bundle_path = tmp_path / "case.zip"

    export_case_bundle(scan_path, bundle_path, include_files=True)

    assert verify_bundle(str(bundle_path)) == []


def test_verify_bundle_requires_every_declared_original(tmp_path):
    source = tmp_path / "evidence.txt"
    source.write_text("evidence", encoding="utf-8")
    record = {
        "path": "evidence.txt",
        "sha256": sha256_file(source),
        "size_bytes": source.stat().st_size,
        "mime": "text/plain",
        "metadata": {},
        "warnings": [],
        "errors": [],
    }
    scan_path = tmp_path / "scan.jsonl"
    scan_path.write_text(json.dumps(record) + "\n", encoding="utf-8")
    bundle_path = tmp_path / "case.zip"
    stripped_path = tmp_path / "stripped.zip"
    export_case_bundle(scan_path, bundle_path, include_files=True)

    _rewrite_zip(bundle_path, stripped_path, remove={"files/evidence.txt"})

    issues = verify_bundle(str(stripped_path))
    assert "missing_bundled_file" in {issue["issue"] for issue in issues}


def test_verify_bundle_binds_manifest_inventory_to_scan(tmp_path):
    source = tmp_path / "evidence.txt"
    source.write_text("evidence", encoding="utf-8")
    record = {
        "path": "evidence.txt",
        "sha256": sha256_file(source),
        "size_bytes": source.stat().st_size,
        "mime": "text/plain",
        "metadata": {},
        "warnings": [],
        "errors": [],
    }
    scan_path = tmp_path / "scan.jsonl"
    scan_path.write_text(json.dumps(record) + "\n", encoding="utf-8")
    bundle_path = tmp_path / "case.zip"
    tampered_path = tmp_path / "tampered.zip"
    export_case_bundle(scan_path, bundle_path, include_files=True)

    with zipfile.ZipFile(bundle_path, "r") as bundle:
        manifest = json.loads(bundle.read("manifest.json"))
    manifest["original_files"][0]["sha256"] = "0" * 64
    _rewrite_zip(
        bundle_path,
        tampered_path,
        replacements={"manifest.json": json.dumps(manifest).encode("utf-8")},
    )

    issues = verify_bundle(str(tampered_path))
    assert "manifest_file_hash_mismatch" in {
        issue["issue"] for issue in issues
    }
