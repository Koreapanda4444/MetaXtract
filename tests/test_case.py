import hashlib
import json
import zipfile

import pytest

import metaxtract.case.verify as verify_module
import metaxtract.core.jsonio as jsonio_module
from metaxtract.case.bundle import export_case_bundle
from metaxtract.case.signing import generate_signing_keypair
from metaxtract.case.verify import verify_bundle, verify_scan
from metaxtract.core.files import sha256_file
from metaxtract.core.jsonio import read_jsonl


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
            "gps_altitude_m": 32.0,
            "exif_artist": "Private Artist",
            "exif_datetime_original": "2026:01:02 03:04:05",
            "docx_author": "Private Person",
            "docx_created": "2026-01-02T03:04:05+00:00",
            "pdf_creation_date": "2026-01-02T03:04:05+00:00",
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


def test_case_bundle_is_deterministic_and_hashes_every_artifact(tmp_path):
    source = tmp_path / "evidence.txt"
    source.write_text("deterministic evidence", encoding="utf-8")
    scan_path = tmp_path / "scan.jsonl"
    scan_path.write_text(
        json.dumps(
            _record(
                "evidence.txt",
                sha256=sha256_file(source),
                size_bytes=source.stat().st_size,
                mime="text/plain",
                metadata={"mtime": 1_760_000_000.0},
            )
        )
        + "\n",
        encoding="utf-8",
    )
    first_bundle = tmp_path / "first.zip"
    second_bundle = tmp_path / "second.zip"

    export_case_bundle(scan_path, first_bundle, include_files=True)
    export_case_bundle(scan_path, second_bundle, include_files=True)

    assert first_bundle.read_bytes() == second_bundle.read_bytes()
    with zipfile.ZipFile(first_bundle, "r") as bundle:
        assert bundle.namelist() == [
            "manifest.json",
            "scan.jsonl",
            "hashes.txt",
            "reports/report.json",
            "files/evidence.txt",
        ]
        assert all(info.date_time == (1980, 1, 1, 0, 0, 0) for info in bundle.infolist())
        assert all((info.external_attr >> 16) & 0o777 == 0o644 for info in bundle.infolist())
        manifest = json.loads(bundle.read("manifest.json"))
        artifacts = {item["path"]: item for item in manifest["artifacts"]}
        assert set(artifacts) == set(bundle.namelist()) - {"manifest.json"}
        for path, item in artifacts.items():
            data = bundle.read(path)
            assert item["size_bytes"] == len(data)
            assert item["sha256"] == hashlib.sha256(data).hexdigest()

    assert verify_bundle(str(first_bundle)) == []


def test_signed_bundle_is_deterministic_and_requires_matching_public_key(tmp_path):
    scan_path = tmp_path / "scan.jsonl"
    scan_path.write_text(json.dumps(_record("evidence.txt")) + "\n", encoding="utf-8")
    private_key = tmp_path / "private.pem"
    public_key = tmp_path / "public.pem"
    wrong_private_key = tmp_path / "wrong-private.pem"
    wrong_public_key = tmp_path / "wrong-public.pem"
    generate_signing_keypair(private_key, public_key)
    generate_signing_keypair(wrong_private_key, wrong_public_key)
    first_bundle = tmp_path / "first.zip"
    second_bundle = tmp_path / "second.zip"

    export_case_bundle(scan_path, first_bundle, signing_key=private_key)
    export_case_bundle(scan_path, second_bundle, signing_key=private_key)

    assert first_bundle.read_bytes() == second_bundle.read_bytes()
    with zipfile.ZipFile(first_bundle, "r") as bundle:
        assert bundle.namelist()[0:2] == ["manifest.json", "signature.json"]
        signature = json.loads(bundle.read("signature.json"))
        assert signature["algorithm"] == "Ed25519"

    unsigned_issues = verify_bundle(str(first_bundle))
    wrong_key_issues = verify_bundle(str(first_bundle), public_key=wrong_public_key)
    assert {issue["issue"] for issue in unsigned_issues} == {"signature_key_required"}
    assert "public_key_mismatch" in {issue["issue"] for issue in wrong_key_issues}
    assert verify_bundle(str(first_bundle), public_key=public_key) == []


def test_signed_bundle_detects_manifest_tampering(tmp_path):
    scan_path = tmp_path / "scan.jsonl"
    scan_path.write_text(json.dumps(_record("evidence.txt")) + "\n", encoding="utf-8")
    private_key = tmp_path / "private.pem"
    public_key = tmp_path / "public.pem"
    generate_signing_keypair(private_key, public_key)
    bundle_path = tmp_path / "case.zip"
    tampered_path = tmp_path / "tampered.zip"
    export_case_bundle(scan_path, bundle_path, signing_key=private_key)

    with zipfile.ZipFile(bundle_path, "r") as bundle:
        manifest = json.loads(bundle.read("manifest.json"))
    manifest["notes"] = "tampered"
    _rewrite_zip(
        bundle_path,
        tampered_path,
        replacements={"manifest.json": json.dumps(manifest).encode("utf-8")},
    )

    issues = verify_bundle(str(tampered_path), public_key=public_key)
    assert "manifest_signature_hash_mismatch" in {issue["issue"] for issue in issues}


def test_unsigned_bundle_rejects_requested_signature_verification(tmp_path):
    scan_path = tmp_path / "scan.jsonl"
    scan_path.write_text(json.dumps(_record("evidence.txt")) + "\n", encoding="utf-8")
    private_key = tmp_path / "private.pem"
    public_key = tmp_path / "public.pem"
    generate_signing_keypair(private_key, public_key)
    bundle_path = tmp_path / "case.zip"
    export_case_bundle(scan_path, bundle_path)

    issues = verify_bundle(str(bundle_path), public_key=public_key)

    assert "missing_signature" in {issue["issue"] for issue in issues}


def test_key_generation_refuses_to_overwrite_existing_keys(tmp_path):
    private_key = tmp_path / "private.pem"
    public_key = tmp_path / "public.pem"
    generate_signing_keypair(private_key, public_key)

    with pytest.raises(FileExistsError, match="refusing to overwrite"):
        generate_signing_keypair(private_key, tmp_path / "new-public.pem")

    assert private_key.stat().st_mode & 0o777 == 0o600


def test_verify_bundle_detects_tampered_artifact(tmp_path):
    scan_path = tmp_path / "scan.jsonl"
    scan_path.write_text(json.dumps(_record("evidence.txt")) + "\n", encoding="utf-8")
    bundle_path = tmp_path / "case.zip"
    tampered_path = tmp_path / "tampered.zip"
    export_case_bundle(scan_path, bundle_path)

    _rewrite_zip(
        bundle_path,
        tampered_path,
        replacements={"reports/report.json": b"{}\n"},
    )

    issues = verify_bundle(str(tampered_path))
    assert "artifact_hash_mismatch" in {issue["issue"] for issue in issues}


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
    assert "manifest_file_hash_mismatch" in {issue["issue"] for issue in issues}


def test_jsonl_reader_bounds_lines_bytes_and_record_count(tmp_path, monkeypatch):
    first = _record("first.txt")
    second = _record("second.txt", sha256="1" * 64)
    scan_path = tmp_path / "scan.jsonl"
    payload = json.dumps(first) + "\n" + json.dumps(second) + "\n"
    scan_path.write_text(payload, encoding="utf-8")

    monkeypatch.setattr(jsonio_module, "MAX_JSONL_LINE_BYTES", 32)
    with pytest.raises(ValueError, match="line 1 exceeds"):
        read_jsonl(scan_path)

    monkeypatch.setattr(jsonio_module, "MAX_JSONL_LINE_BYTES", 1024 * 1024)
    monkeypatch.setattr(jsonio_module, "MAX_JSONL_BYTES", 32)
    with pytest.raises(ValueError, match="JSONL exceeds"):
        read_jsonl(scan_path)

    monkeypatch.setattr(jsonio_module, "MAX_JSONL_BYTES", 1024 * 1024)
    monkeypatch.setattr(jsonio_module, "MAX_JSONL_RECORDS", 1)
    with pytest.raises(ValueError, match="exceeds 1 records"):
        read_jsonl(scan_path)


def test_bundle_verifier_bounds_member_count_and_sizes(tmp_path, monkeypatch):
    bundle_path = tmp_path / "limits.zip"
    with zipfile.ZipFile(bundle_path, "w", compression=zipfile.ZIP_STORED) as bundle:
        bundle.writestr("one.bin", b"a" * 32)
        bundle.writestr("two.bin", b"b" * 32)
        bundle.writestr("three.bin", b"c" * 32)

    monkeypatch.setattr(verify_module, "MAX_BUNDLE_MEMBERS", 2)
    issues = verify_bundle(str(bundle_path))
    assert "too_many_zip_entries" in {issue["issue"] for issue in issues}

    monkeypatch.setattr(verify_module, "MAX_BUNDLE_MEMBERS", 10)
    monkeypatch.setattr(verify_module, "MAX_BUNDLE_MEMBER_BYTES", 16)
    issues = verify_bundle(str(bundle_path))
    assert "zip_member_too_large" in {issue["issue"] for issue in issues}

    monkeypatch.setattr(verify_module, "MAX_BUNDLE_MEMBER_BYTES", 1024)
    monkeypatch.setattr(verify_module, "MAX_BUNDLE_TOTAL_BYTES", 64)
    issues = verify_bundle(str(bundle_path))
    assert "zip_total_size_exceeded" in {issue["issue"] for issue in issues}


def test_bundle_verifier_rejects_extreme_compression_ratio(tmp_path):
    bundle_path = tmp_path / "compression-bomb.zip"
    with zipfile.ZipFile(bundle_path, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
        bundle.writestr("files/repeated.bin", b"0" * 1024 * 1024)

    issues = verify_bundle(str(bundle_path))

    assert "suspicious_compression_ratio" in {issue["issue"] for issue in issues}


def test_bundle_verifier_bounds_archive_bytes(tmp_path, monkeypatch):
    bundle_path = tmp_path / "archive.zip"
    with zipfile.ZipFile(bundle_path, "w") as bundle:
        bundle.writestr("entry.txt", b"data")
    monkeypatch.setattr(verify_module, "MAX_BUNDLE_ARCHIVE_BYTES", 1)

    issues = verify_bundle(str(bundle_path))

    assert "bundle_archive_too_large" in {issue["issue"] for issue in issues}
