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


def test_help_and_doctor_work_without_feature_dependencies():
    help_result = _run_cli_without_site_packages("--help")
    doctor_result = _run_cli_without_site_packages("doctor")

    assert help_result.returncode == 0, help_result.stderr
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


def test_commands_reject_invalid_records_consistently(tmp_path):
    invalid_scan = tmp_path / "invalid.jsonl"
    invalid_scan.write_text(
        json.dumps(
            {
                "path": "bad.txt",
                "sha256": "not-a-hash",
                "size_bytes": -1,
                "mime": 42,
                "metadata": [],
                "warnings": "warning",
                "errors": [1],
            }
        )
        + "\n",
        encoding="utf-8",
    )

    commands = [
        ("report", str(invalid_scan)),
        ("report-html", str(invalid_scan)),
        ("diff", str(invalid_scan), str(invalid_scan)),
        ("export-case", str(invalid_scan), str(tmp_path / "case.zip")),
        ("verify", str(invalid_scan), str(tmp_path)),
    ]
    for command in commands:
        result = _run_cli(*command)
        output = result.stdout + result.stderr
        assert result.returncode == 1, output
        assert "invalid_hash" in output
        assert "invalid_size" in output
        assert "Traceback" not in output


def test_commands_reject_duplicate_record_paths(tmp_path):
    scan_path = tmp_path / "duplicates.jsonl"
    record = {
        "path": "Evidence.txt",
        "sha256": "0" * 64,
        "size_bytes": 0,
        "mime": "text/plain",
        "metadata": {},
        "warnings": [],
        "errors": [],
    }
    duplicate = {**record, "path": "evidence.txt"}
    scan_path.write_text(
        json.dumps(record) + "\n" + json.dumps(duplicate) + "\n",
        encoding="utf-8",
    )

    commands = [
        ("report", str(scan_path)),
        ("diff", str(scan_path), str(scan_path)),
        ("export-case", str(scan_path), str(tmp_path / "case.zip")),
    ]
    for command in commands:
        result = _run_cli(*command)
        output = result.stdout + result.stderr
        assert result.returncode == 1, output
        assert "duplicate_path" in output
        assert "Traceback" not in output


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
