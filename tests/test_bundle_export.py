
import zipfile
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
