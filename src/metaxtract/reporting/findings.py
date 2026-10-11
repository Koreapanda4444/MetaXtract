from __future__ import annotations

import math
from collections import Counter
from dataclasses import asdict, dataclass
from datetime import datetime
from pathlib import PurePosixPath
from typing import Any, Iterable, Mapping

from ..core.models import require_valid_records
from .timeline import build_timeline


@dataclass(frozen=True)
class FindingEvidence:
    field: str
    value: Any


@dataclass(frozen=True)
class Finding:
    path: str
    rule_id: str
    category: str
    severity: str
    title: str
    detail: str
    evidence: tuple[FindingEvidence, ...]


_IDENTITY_FIELDS = {
    "author",
    "artist",
    "camera_owner_name",
    "cameraownername",
    "exif_artist",
    "last_modified_by",
    "lastmodifiedby",
    "owner",
    "pdf_author",
    "docx_author",
    "docx_last_modified_by",
    "user",
    "username",
}
_DEVICE_FIELDS = {
    "exif_lens_model",
    "exif_make",
    "exif_model",
    "lens_make",
    "lens_model",
    "lensmake",
    "lensmodel",
    "make",
    "model",
}
_SOFTWARE_FIELDS = {
    "application",
    "creator_tool",
    "creatortool",
    "encoder",
    "encoding_tool",
    "encodingtool",
    "exif_software",
    "pdf_creator",
    "pdf_producer",
    "producer",
    "software",
    "writing_application",
    "writingapplication",
}
_MIME_RULES = {
    ".jpg": lambda mime: mime == "image/jpeg",
    ".jpeg": lambda mime: mime == "image/jpeg",
    ".png": lambda mime: mime == "image/png",
    ".pdf": lambda mime: mime == "application/pdf",
    ".docx": lambda mime: (
        mime == "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    ),
    ".mp4": lambda mime: mime.startswith("video/"),
    ".mov": lambda mime: mime.startswith("video/"),
    ".m4v": lambda mime: mime.startswith("video/"),
}
_SEVERITY_ORDER = {"high": 0, "medium": 1, "low": 2}


def _present(value: Any) -> bool:
    return value not in (None, "", [], {})


def _walk_metadata(
    value: Any,
    prefix: str = "metadata",
) -> Iterable[tuple[str, str, Any]]:
    if isinstance(value, Mapping):
        for key, nested in value.items():
            name = str(key)
            field = f"{prefix}.{name}"
            yield field, name.casefold(), nested
            yield from _walk_metadata(nested, field)
    elif isinstance(value, list):
        for index, nested in enumerate(value):
            yield from _walk_metadata(nested, f"{prefix}.{index}")


def _field_evidence(
    metadata: Mapping[str, Any],
    names: set[str],
) -> tuple[FindingEvidence, ...]:
    evidence = []
    for field, name, value in _walk_metadata(metadata):
        normalized = name.replace("-", "_").replace(" ", "_")
        if normalized in names and _present(value):
            evidence.append(FindingEvidence(field, value))
    return tuple(sorted(evidence, key=lambda item: item.field.casefold()))


def _serial_evidence(metadata: Mapping[str, Any]) -> tuple[FindingEvidence, ...]:
    evidence = [
        FindingEvidence(field, value)
        for field, name, value in _walk_metadata(metadata)
        if "serial" in name and _present(value)
    ]
    return tuple(sorted(evidence, key=lambda item: item.field.casefold()))


def _gps_evidence(
    metadata: Mapping[str, Any],
) -> tuple[
    FindingEvidence | None,
    FindingEvidence | None,
    tuple[FindingEvidence, ...],
]:
    latitude = None
    longitude = None
    evidence = []
    for key, value in metadata.items():
        name = str(key)
        normalized = name.casefold()
        if normalized in {"gps_latitude", "gps_longitude", "gps_altitude_m", "gps_altitude"}:
            item = FindingEvidence(f"metadata.{name}", value)
            evidence.append(item)
            if normalized == "gps_latitude":
                latitude = item
            elif normalized == "gps_longitude":
                longitude = item
        elif normalized == "gps":
            if not isinstance(value, Mapping):
                evidence.append(FindingEvidence(f"metadata.{name}", value))
                continue
            for nested_key, nested_value in value.items():
                nested_name = str(nested_key)
                nested_normalized = nested_name.casefold()
                item = FindingEvidence(f"metadata.{name}.{nested_name}", nested_value)
                if nested_normalized in {
                    "latitude",
                    "lat",
                    "gpslatitude",
                    "longitude",
                    "lon",
                    "lng",
                    "gpslongitude",
                    "altitude",
                    "altitude_m",
                    "gpsaltitude",
                }:
                    evidence.append(item)
                if nested_normalized in {"latitude", "lat", "gpslatitude"}:
                    latitude = item
                elif nested_normalized in {"longitude", "lon", "lng", "gpslongitude"}:
                    longitude = item
    return latitude, longitude, tuple(sorted(evidence, key=lambda item: item.field.casefold()))


def _number(value: Any) -> float | None:
    if isinstance(value, bool):
        return None
    try:
        result = float(value)
    except (TypeError, ValueError, OverflowError):
        return None
    return result if math.isfinite(result) else None


def _privacy_findings(record: dict[str, Any]) -> list[Finding]:
    path = str(record["path"])
    metadata = record["metadata"]
    findings = []

    latitude, longitude, gps = _gps_evidence(metadata)
    if gps:
        findings.append(
            Finding(
                path,
                "privacy.gps",
                "privacy",
                "high",
                "GPS location metadata",
                "The file exposes recorded geographic coordinates or related GPS fields.",
                gps,
            )
        )

    identity = _field_evidence(metadata, _IDENTITY_FIELDS)
    if identity:
        findings.append(
            Finding(
                path,
                "privacy.identity",
                "privacy",
                "medium",
                "Author or user identity",
                "Author, owner, artist, or user metadata may identify a person or account.",
                identity,
            )
        )

    device = _field_evidence(metadata, _DEVICE_FIELDS)
    if device:
        findings.append(
            Finding(
                path,
                "privacy.device",
                "privacy",
                "medium",
                "Device identity",
                "Make, model, or lens metadata may identify the recording device family.",
                device,
            )
        )

    serials = _serial_evidence(metadata)
    if serials:
        findings.append(
            Finding(
                path,
                "privacy.serial",
                "privacy",
                "high",
                "Device serial number",
                "A serial field may uniquely identify the device or hardware component.",
                serials,
            )
        )

    software = _field_evidence(metadata, _SOFTWARE_FIELDS)
    if software:
        findings.append(
            Finding(
                path,
                "privacy.software",
                "privacy",
                "low",
                "Software fingerprint",
                "Creator, producer, encoder, or software metadata reveals the tool chain.",
                software,
            )
        )

    if gps:
        problems = []
        if latitude is None or longitude is None:
            problems.append("latitude/longitude pair is incomplete")
        else:
            latitude_value = _number(latitude.value)
            longitude_value = _number(longitude.value)
            if latitude_value is None or longitude_value is None:
                problems.append("coordinates are not finite numbers")
            elif not -90 <= latitude_value <= 90 or not -180 <= longitude_value <= 180:
                problems.append("coordinates are outside valid latitude/longitude ranges")
        altitude_values = [
            item.value
            for item in gps
            if item.field.casefold().endswith(
                ("gps_altitude", "gps_altitude_m", ".altitude", ".altitude_m", ".gpsaltitude")
            )
        ]
        if any(_number(value) is None for value in altitude_values):
            problems.append("altitude is not a finite number")
        if problems:
            findings.append(
                Finding(
                    path,
                    "anomaly.invalid_gps",
                    "anomaly",
                    "high",
                    "Invalid GPS metadata",
                    "; ".join(problems).capitalize() + ".",
                    gps,
                )
            )
    return findings


def _extension_finding(record: dict[str, Any]) -> Finding | None:
    path = str(record["path"])
    suffix = PurePosixPath(path).suffix.casefold()
    predicate = _MIME_RULES.get(suffix)
    mime = str(record["mime"]).casefold()
    if predicate is None or predicate(mime):
        return None
    return Finding(
        path,
        "anomaly.extension_mime_mismatch",
        "anomaly",
        "medium",
        "Extension and media type disagree",
        f"The {suffix or '[none]'} extension does not match the recorded {mime} media type.",
        (
            FindingEvidence("record.path", path),
            FindingEvidence("record.mime", record["mime"]),
        ),
    )


def _timestamp(value: str) -> datetime:
    normalized = value[:-1] + "+00:00" if value.endswith("Z") else value
    return datetime.fromisoformat(normalized)


def _date_findings(record: dict[str, Any]) -> list[Finding]:
    timeline = build_timeline([record])
    events = {event.source_field: event for event in timeline}
    pairs = (
        ("pdf_creation_date", "pdf_modification_date"),
        ("docx_created", "docx_modified"),
    )
    findings = []
    for created_field, modified_field in pairs:
        created = events.get(created_field)
        modified = events.get(modified_field)
        if created is None or modified is None:
            continue
        if created.timezone_known != modified.timezone_known:
            continue
        if _timestamp(created.timestamp) <= _timestamp(modified.timestamp):
            continue
        findings.append(
            Finding(
                str(record["path"]),
                "anomaly.date_order",
                "anomaly",
                "medium",
                "Creation date is after modification date",
                "Comparable metadata dates are in an unexpected chronological order.",
                (
                    FindingEvidence(f"metadata.{created_field}", created.source_value),
                    FindingEvidence(f"metadata.{modified_field}", modified.source_value),
                ),
            )
        )

    filesystem_modified = events.get("mtime")
    if filesystem_modified is not None:
        for event in timeline:
            if event.source_field == "mtime" or event.event not in {
                "captured",
                "created",
                "encoded",
                "tagged",
            }:
                continue
            if event.timezone_known != filesystem_modified.timezone_known:
                continue
            if _timestamp(event.timestamp) <= _timestamp(filesystem_modified.timestamp):
                continue
            findings.append(
                Finding(
                    str(record["path"]),
                    "anomaly.date_after_filesystem",
                    "anomaly",
                    "medium",
                    "Metadata date is after filesystem modification",
                    "A comparable capture or creation date is later than the recorded file mtime.",
                    (
                        FindingEvidence(f"metadata.{event.source_field}", event.source_value),
                        FindingEvidence("metadata.mtime", filesystem_modified.source_value),
                    ),
                )
            )
    return findings


def _read_failure_finding(record: dict[str, Any]) -> Finding | None:
    errors = record["errors"]
    warnings = [
        (index, value)
        for index, value in enumerate(record["warnings"])
        if any(
            marker in value.casefold()
            for marker in ("failed", "invalid", "not_found", "nonzero", "password", "timeout")
        )
    ]
    if not errors and not warnings:
        return None
    evidence = [
        FindingEvidence(f"record.errors.{index}", value) for index, value in enumerate(errors)
    ]
    evidence.extend(
        FindingEvidence(f"record.warnings.{index}", value) for index, value in warnings
    )
    return Finding(
        str(record["path"]),
        "processing.read_failure",
        "processing",
        "high" if errors else "medium",
        "File processing failure" if errors else "Incomplete file processing",
        "One or more scan stages could not fully read or process the file.",
        tuple(evidence),
    )


def classify_findings(records: Iterable[dict[str, Any]]) -> list[Finding]:
    findings = []
    for record in require_valid_records(records):
        findings.extend(_privacy_findings(record))
        extension = _extension_finding(record)
        if extension is not None:
            findings.append(extension)
        findings.extend(_date_findings(record))
        read_failure = _read_failure_finding(record)
        if read_failure is not None:
            findings.append(read_failure)
    return sorted(
        findings,
        key=lambda item: (
            _SEVERITY_ORDER[item.severity],
            item.path.casefold(),
            item.rule_id,
        ),
    )


def findings_document(records: Iterable[dict[str, Any]]) -> dict[str, Any]:
    findings = classify_findings(records)
    categories = Counter(item.category for item in findings)
    severities = Counter(item.severity for item in findings)
    rules = Counter(item.rule_id for item in findings)
    return {
        "total_findings": len(findings),
        "files_with_findings": len({item.path for item in findings}),
        "by_category": dict(sorted(categories.items())),
        "by_severity": dict(sorted(severities.items())),
        "by_rule": dict(sorted(rules.items())),
        "findings": [asdict(item) for item in findings],
    }


def _values(metadata: Mapping[str, Any], keys: Iterable[str]) -> list[str]:
    values = []
    seen = set()
    for key in keys:
        value = metadata.get(key)
        if value in (None, ""):
            continue
        text = str(value)
        if text not in seen:
            values.append(text)
            seen.add(text)
    return values


def collect_findings(records: list[dict[str, Any]]) -> dict[str, Any]:
    gps_files = []
    authors = Counter()
    producers = Counter()
    models = Counter()

    for record in records:
        metadata = record.get("metadata") or {}
        _latitude, _longitude, gps = _gps_evidence(metadata)
        if gps:
            gps_files.append(str(record.get("path") or ""))
        authors.update(_values(metadata, ("author", "pdf_author", "docx_author")))
        producers.update(
            _values(
                metadata,
                ("producer", "pdf_producer", "pdf_creator", "software", "exif_software"),
            )
        )
        models.update(_values(metadata, ("model", "exif_model")))

    return {
        "gps_files": gps_files,
        "authors": authors,
        "producers": producers,
        "models": models,
        "classified": findings_document(records),
    }
