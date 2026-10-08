import json
import os
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
FIXTURES = Path(__file__).parent / "fixtures"


def _run_cli(*arguments: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, "-m", "metaxtract", *arguments],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )


def _run_cli_without_site_packages(*arguments: str) -> subprocess.CompletedProcess:
    env = os.environ.copy()
    env["PYTHONPATH"] = str(ROOT / "src")
    return subprocess.run(
        [sys.executable, "-S", "-m", "metaxtract", *arguments],
        cwd=ROOT,
        env=env,
        check=False,
        capture_output=True,
        text=True,
    )


def test_help_version_and_doctor_work_without_feature_dependencies():
    help_result = _run_cli_without_site_packages("--help")
    version_result = _run_cli_without_site_packages("--version")
    doctor_result = _run_cli_without_site_packages("doctor")

    assert help_result.returncode == 0, help_result.stderr
    assert version_result.returncode == 0, version_result.stderr
    assert version_result.stdout.strip().startswith("metaxtract ")
    assert doctor_result.returncode == 1
    assert "MISSING Pillow" in doctor_result.stdout
    assert "Traceback" not in doctor_result.stderr


def test_runtime_and_usage_failures_have_stable_exit_codes(tmp_path):
    runtime_failure = _run_cli("report", str(tmp_path / "missing.jsonl"))
    usage_failure = _run_cli("scan")

    assert runtime_failure.returncode == 1
    assert "report failed:" in runtime_failure.stderr
    assert "Traceback" not in runtime_failure.stderr
    assert usage_failure.returncode == 2


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
