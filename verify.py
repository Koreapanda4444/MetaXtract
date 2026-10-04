from __future__ import annotations

import hashlib
import json
import re
import zipfile
from pathlib import Path
from typing import Any, Dict, Iterable, List, Tuple

from path_safety import normalize_relative_path, resolve_source
from report import build_report_from_rows
from rules_privacy import PRIVACY_KEYS
from utils import read_jsonl, sha256_file


_SHA256_PATTERN = re.compile(r"^[0-9a-fA-F]{64}$")
_REQUIRED_BUNDLE_ENTRIES = {
    "manifest.json",
    "scan.jsonl",
    "hashes.txt",
    "reports/report.json",
}


def _issue(path: str, issue: str, detail: str | None = None) -> Dict[str, str]:
    result = {"path": path, "issue": issue}
    if detail:
        result["detail"] = detail
    return result


def _validate_rows(rows: Iterable[Any]) -> Tuple[List[Dict[str, Any]], List[Dict[str, str]]]:
    valid_rows = []
    issues = []
    seen_paths = set()

    for index, row in enumerate(rows, start=1):
        if not isinstance(row, dict):
            issues.append(_issue(f"row:{index}", "invalid_record"))
            continue
        try:
            relative_path = normalize_relative_path(row.get("path"))
        except ValueError as exc:
            issues.append(_issue(f"row:{index}", "invalid_path", str(exc)))
            continue

        collision_key = relative_path.casefold()
        if collision_key in seen_paths:
            issues.append(_issue(relative_path, "duplicate_path"))
            continue
        seen_paths.add(collision_key)

        expected_hash = row.get("sha256")
        if not isinstance(expected_hash, str) or not _SHA256_PATTERN.fullmatch(expected_hash):
            issues.append(_issue(relative_path, "invalid_hash"))

        normalized_row = dict(row)
        normalized_row["path"] = relative_path
        valid_rows.append(normalized_row)

    return valid_rows, issues


def _verify_rows_against_base(
    rows: List[Dict[str, Any]],
    files_base: str | Path,
) -> List[Dict[str, str]]:
    issues = []
    base = Path(files_base)
    if not base.exists():
        return [_issue(str(base), "missing_base")]
    if not base.is_dir():
        return [_issue(str(base), "invalid_base")]
    base = base.resolve(strict=True)

    for row in rows:
        relative_path = row["path"]
        try:
            source = resolve_source(base, relative_path)
        except FileNotFoundError:
            issues.append(_issue(relative_path, "missing"))
            continue
        except IsADirectoryError:
            issues.append(_issue(relative_path, "not_file"))
            continue
        except ValueError as exc:
            issue_name = "symlink" if "symbolic" in str(exc) else "unsafe_path"
            issues.append(_issue(relative_path, issue_name, str(exc)))
            continue

        expected_size = row.get("size_bytes")
        if isinstance(expected_size, int) and source.stat().st_size != expected_size:
            issues.append(_issue(relative_path, "size_mismatch"))

        expected_hash = row.get("sha256")
        if not isinstance(expected_hash, str) or not _SHA256_PATTERN.fullmatch(expected_hash):
            continue
        try:
            actual_hash = sha256_file(source)
        except OSError as exc:
            issues.append(_issue(relative_path, "read_failed", type(exc).__name__))
            continue
        if actual_hash.lower() != expected_hash.lower():
            issues.append(_issue(relative_path, "hash_mismatch"))

    return issues


def verify_scan(scan_jsonl_path: str, files_base: str) -> List[Dict[str, str]]:
    try:
        rows = read_jsonl(scan_jsonl_path)
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        return [_issue(str(scan_jsonl_path), "invalid_scan", str(exc))]

    valid_rows, issues = _validate_rows(rows)
    issues.extend(_verify_rows_against_base(valid_rows, files_base))
    return issues


def _read_member(zf: zipfile.ZipFile, info: zipfile.ZipInfo) -> bytes:
    with zf.open(info, "r") as source:
        return source.read()


def _parse_jsonl_bytes(data: bytes) -> List[Any]:
    rows = []
    text = data.decode("utf-8")
    for line_number, line in enumerate(text.splitlines(), start=1):
        if not line.strip():
            continue
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError as exc:
            raise ValueError(f"invalid JSONL at line {line_number}: {exc.msg}") from exc
    return rows


def _parse_hashes(data: bytes) -> Tuple[Dict[str, str], List[Dict[str, str]]]:
    hashes = {}
    issues = []
    seen_paths = set()
    try:
        text = data.decode("utf-8")
    except UnicodeError as exc:
        return {}, [_issue("hashes.txt", "invalid_encoding", str(exc))]

    for line_number, line in enumerate(text.splitlines(), start=1):
        if not line:
            continue
        if line.count("\t") != 1:
            issues.append(_issue(f"hashes.txt:{line_number}", "invalid_hash_line"))
            continue
        expected_hash, raw_path = line.split("\t", 1)
        try:
            relative_path = normalize_relative_path(raw_path)
        except ValueError as exc:
            issues.append(
                _issue(f"hashes.txt:{line_number}", "invalid_path", str(exc))
            )
            continue
        collision_key = relative_path.casefold()
        if collision_key in seen_paths:
            issues.append(_issue(relative_path, "duplicate_hash_path"))
            continue
        seen_paths.add(collision_key)
        if not _SHA256_PATTERN.fullmatch(expected_hash):
            issues.append(_issue(relative_path, "invalid_hash"))
            continue
        hashes[relative_path] = expected_hash.lower()
    return hashes, issues


def _zip_member_sha256(zf: zipfile.ZipFile, info: zipfile.ZipInfo) -> str:
    digest = hashlib.sha256()
    with zf.open(info, "r") as source:
        while True:
            chunk = source.read(1024 * 1024)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def _contains_private_metadata(value: Any) -> bool:
    if isinstance(value, dict):
        if any(key in PRIVACY_KEYS for key in value):
            return True
        return any(_contains_private_metadata(item) for item in value.values())
    if isinstance(value, list):
        return any(_contains_private_metadata(item) for item in value)
    return False


def verify_bundle(
    bundle_zip_path: str,
    files_base: str | None = None,
) -> List[Dict[str, str]]:
    issues = []
    try:
        zf = zipfile.ZipFile(bundle_zip_path, "r")
    except (OSError, zipfile.BadZipFile) as exc:
        return [_issue(str(bundle_zip_path), "invalid_bundle", str(exc))]

    with zf:
        members = {}
        seen_names = set()
        for info in zf.infolist():
            try:
                name = normalize_relative_path(info.filename)
            except ValueError as exc:
                issues.append(_issue(info.filename, "unsafe_zip_path", str(exc)))
                continue
            collision_key = name.casefold()
            if collision_key in seen_names:
                issues.append(_issue(name, "duplicate_zip_entry"))
                continue
            seen_names.add(collision_key)
            members[name] = info

        for required_name in sorted(_REQUIRED_BUNDLE_ENTRIES):
            if required_name not in members:
                issues.append(_issue(required_name, "missing_bundle_entry"))

        for name in members:
            if name not in _REQUIRED_BUNDLE_ENTRIES and not name.startswith("files/"):
                issues.append(_issue(name, "unexpected_bundle_entry"))

        try:
            bad_crc = zf.testzip()
        except (OSError, RuntimeError, zipfile.BadZipFile) as exc:
            issues.append(_issue(str(bundle_zip_path), "bundle_read_failed", str(exc)))
            bad_crc = None
        if bad_crc:
            issues.append(_issue(bad_crc, "crc_mismatch"))

        rows = []
        if "scan.jsonl" in members:
            try:
                rows = _parse_jsonl_bytes(_read_member(zf, members["scan.jsonl"]))
            except (OSError, UnicodeError, ValueError, zipfile.BadZipFile) as exc:
                issues.append(_issue("scan.jsonl", "invalid_scan", str(exc)))

        valid_rows, row_issues = _validate_rows(rows)
        issues.extend(row_issues)
        scan_hashes = {
            row["path"]: str(row.get("sha256") or "").lower()
            for row in valid_rows
        }

        hash_entries = {}
        if "hashes.txt" in members:
            try:
                hash_entries, hash_issues = _parse_hashes(
                    _read_member(zf, members["hashes.txt"])
                )
                issues.extend(hash_issues)
            except (OSError, zipfile.BadZipFile) as exc:
                issues.append(_issue("hashes.txt", "read_failed", str(exc)))

        for path, expected_hash in scan_hashes.items():
            if path not in hash_entries:
                issues.append(_issue(path, "missing_hash_entry"))
            elif hash_entries[path] != expected_hash:
                issues.append(_issue(path, "hash_list_mismatch"))
        for path in hash_entries.keys() - scan_hashes.keys():
            issues.append(_issue(path, "unexpected_hash_entry"))

        file_members = {
            name.removeprefix("files/"): info
            for name, info in members.items()
            if name.startswith("files/")
        }
        if file_members:
            for path, expected_hash in scan_hashes.items():
                info = file_members.get(path)
                if info is None:
                    issues.append(_issue(path, "missing_bundled_file"))
                    continue
                if not _SHA256_PATTERN.fullmatch(expected_hash):
                    continue
                try:
                    actual_hash = _zip_member_sha256(zf, info)
                except (OSError, RuntimeError, zipfile.BadZipFile) as exc:
                    issues.append(_issue(path, "bundled_file_read_failed", str(exc)))
                    continue
                if actual_hash != expected_hash:
                    issues.append(_issue(path, "bundled_file_hash_mismatch"))
            for path in file_members.keys() - scan_hashes.keys():
                issues.append(_issue(path, "unexpected_bundled_file"))
        elif files_base is not None:
            issues.extend(_verify_rows_against_base(valid_rows, files_base))

        if "manifest.json" in members:
            try:
                manifest = json.loads(_read_member(zf, members["manifest.json"]))
                if not isinstance(manifest, dict):
                    raise ValueError("manifest must be a JSON object")
                if manifest.get("record_count") != len(rows):
                    issues.append(_issue("manifest.json", "record_count_mismatch"))
                expected_hashes = [row.get("sha256", "") for row in valid_rows]
                if manifest.get("hashes") != expected_hashes:
                    issues.append(_issue("manifest.json", "manifest_hashes_mismatch"))
                if manifest.get("redacted") is True and any(
                    _contains_private_metadata(row.get("metadata") or {})
                    for row in valid_rows
                ):
                    issues.append(_issue("scan.jsonl", "redaction_leak"))
            except (OSError, UnicodeError, ValueError, json.JSONDecodeError) as exc:
                issues.append(_issue("manifest.json", "invalid_manifest", str(exc)))

        if "reports/report.json" in members:
            try:
                report = json.loads(_read_member(zf, members["reports/report.json"]))
                if report != build_report_from_rows(valid_rows):
                    issues.append(_issue("reports/report.json", "report_mismatch"))
            except (OSError, UnicodeError, json.JSONDecodeError) as exc:
                issues.append(_issue("reports/report.json", "invalid_report", str(exc)))

    return issues


def verify_bundle_hashes(bundle_zip_path: str, files_base: str = None):
    return verify_bundle(bundle_zip_path, files_base)
