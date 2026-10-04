import json

from bundle_export import export_case_bundle
from utils import sha256_file
from verify import verify_bundle, verify_scan


def test_verify_scan_detects_changes_and_duplicate_paths(tmp_path):
    source = tmp_path / "evidence.txt"
    source.write_text("original", encoding="utf-8")
    record = {
        "path": "evidence.txt",
        "sha256": sha256_file(source),
        "size_bytes": source.stat().st_size,
    }
    scan_path = tmp_path / "scan.jsonl"
    scan_path.write_text(
        json.dumps(record) + "\n" + json.dumps(record) + "\n",
        encoding="utf-8",
    )
    source.write_text("changed", encoding="utf-8")

    issue_names = {issue["issue"] for issue in verify_scan(str(scan_path), str(tmp_path))}

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
