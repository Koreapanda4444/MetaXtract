import json
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
FIXTURES = Path(__file__).parent / "fixtures"


def _run_cli(*arguments: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, "cli.py", *arguments],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )


def test_scan_report_export_and_verify_workflow(tmp_path):
    evidence_dir = tmp_path / "evidence"
    evidence_dir.mkdir()
    shutil.copy2(FIXTURES / "sample.pdf", evidence_dir / "sample.pdf")
    scan_path = tmp_path / "scan.jsonl"
    report_path = tmp_path / "report.json"
    bundle_path = tmp_path / "case.zip"

    scan = _run_cli(
        "scan",
        str(evidence_dir),
        "--out",
        str(scan_path),
        "--cache",
        "off",
    )
    assert scan.returncode == 0, scan.stderr

    report = _run_cli("report", str(scan_path), "--out", str(report_path))
    assert report.returncode == 0, report.stderr
    assert json.loads(report_path.read_text(encoding="utf-8"))["total_files"] == 1

    export = _run_cli(
        "export-case",
        str(scan_path),
        str(bundle_path),
        "--include-files",
        "--files-base",
        str(evidence_dir),
    )
    assert export.returncode == 0, export.stderr

    verify_scan = _run_cli("verify", str(scan_path), str(evidence_dir))
    assert verify_scan.returncode == 0, verify_scan.stdout + verify_scan.stderr
    assert verify_scan.stdout.strip() == "OK"

    verify_bundle = _run_cli("verify-bundle", str(bundle_path))
    assert verify_bundle.returncode == 0, verify_bundle.stdout + verify_bundle.stderr
    assert verify_bundle.stdout.strip() == "OK"
