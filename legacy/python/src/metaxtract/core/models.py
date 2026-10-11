from __future__ import annotations

import re
from dataclasses import dataclass, field
from pathlib import PurePosixPath, PureWindowsPath
from typing import Any, Dict, Iterable, List


SHA256_PATTERN = re.compile(r"^[0-9a-fA-F]{64}$")


@dataclass(frozen=True)
class RecordValidationIssue:
    index: int
    path: str
    issue: str
    detail: str | None = None


class RecordValidationError(ValueError):
    def __init__(self, issues: Iterable[RecordValidationIssue]):
        self.issues = tuple(issues)
        summary = "; ".join(
            f"{item.path}: {item.issue}"
            + (f" ({item.detail})" if item.detail else "")
            for item in self.issues
        )
        super().__init__(f"invalid scan records: {summary}")


def normalize_relative_path(value: object) -> str:
    if not isinstance(value, str) or not value:
        raise ValueError("record has an invalid path")
    if any(char in value for char in "\x00\r\n\t"):
        raise ValueError(f"path contains control characters: {value!r}")
    if PureWindowsPath(value).drive:
        raise ValueError(f"absolute path is not allowed: {value}")

    normalized = value.replace("\\", "/")
    path = PurePosixPath(normalized)
    parts = normalized.split("/")
    if path.is_absolute() or any(part in {"", ".", ".."} for part in parts):
        raise ValueError(f"unsafe relative path: {value}")
    return path.as_posix()


def validate_scan_records(
    rows: Iterable[Any],
) -> tuple[List[Dict[str, Any]], List[RecordValidationIssue]]:
    valid_rows = []
    issues = []
    seen_paths = set()

    for index, row in enumerate(rows, start=1):
        location = f"row:{index}"
        if not isinstance(row, dict):
            issues.append(RecordValidationIssue(index, location, "invalid_record"))
            continue

        try:
            relative_path = normalize_relative_path(row.get("path"))
        except ValueError as exc:
            issues.append(
                RecordValidationIssue(index, location, "invalid_path", str(exc))
            )
            continue

        collision_key = relative_path.casefold()
        if collision_key in seen_paths:
            issues.append(
                RecordValidationIssue(index, relative_path, "duplicate_path")
            )
            continue
        seen_paths.add(collision_key)

        row_issues = []
        sha256 = row.get("sha256")
        if not isinstance(sha256, str) or not SHA256_PATTERN.fullmatch(sha256):
            row_issues.append(
                RecordValidationIssue(index, relative_path, "invalid_hash")
            )

        size_bytes = row.get("size_bytes")
        if type(size_bytes) is not int or size_bytes < 0:
            row_issues.append(
                RecordValidationIssue(index, relative_path, "invalid_size")
            )

        mime = row.get("mime")
        if not isinstance(mime, str) or not mime.strip():
            row_issues.append(
                RecordValidationIssue(index, relative_path, "invalid_mime")
            )

        metadata = row.get("metadata")
        if not isinstance(metadata, dict):
            row_issues.append(
                RecordValidationIssue(index, relative_path, "invalid_metadata")
            )

        warnings = row.get("warnings")
        if not isinstance(warnings, list) or not all(
            isinstance(item, str) for item in warnings
        ):
            row_issues.append(
                RecordValidationIssue(index, relative_path, "invalid_warnings")
            )

        errors = row.get("errors")
        if not isinstance(errors, list) or not all(
            isinstance(item, str) for item in errors
        ):
            row_issues.append(
                RecordValidationIssue(index, relative_path, "invalid_errors")
            )

        if row_issues:
            issues.extend(row_issues)
            continue

        normalized_row = dict(row)
        normalized_row.update(
            {
                "path": relative_path,
                "sha256": sha256.lower(),
                "metadata": dict(metadata),
                "warnings": list(warnings),
                "errors": list(errors),
            }
        )
        valid_rows.append(normalized_row)

    return valid_rows, issues


def require_valid_records(rows: Iterable[Any]) -> List[Dict[str, Any]]:
    valid_rows, issues = validate_scan_records(rows)
    if issues:
        raise RecordValidationError(issues)
    return valid_rows


@dataclass(frozen=True)
class ScanRecord:
    path: str
    mime: str
    size_bytes: int
    sha256: str
    metadata: Dict[str, Any] = field(default_factory=dict)
    warnings: List[str] = field(default_factory=list)
    errors: List[str] = field(default_factory=list)
