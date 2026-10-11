from __future__ import annotations

import re
from dataclasses import asdict, dataclass
from datetime import UTC, datetime, timedelta, timezone
from typing import Any, Iterable, Mapping

from ..core.models import require_valid_records


@dataclass(frozen=True)
class TimelineEvent:
    timestamp: str
    path: str
    event: str
    source_field: str
    source_kind: str
    source_value: Any
    timezone_known: bool
    precision: str


_FIELD_SPECS = (
    ("mtime", "modified", "filesystem"),
    ("exif_datetime_original", "captured", "exif"),
    ("pdf_creation_date", "created", "pdf"),
    ("pdf_modification_date", "modified", "pdf"),
    ("docx_created", "created", "docx"),
    ("docx_modified", "modified", "docx"),
    ("docx_last_printed", "printed", "docx"),
)
_VIDEO_DATE_FIELDS = {
    "creation_time": "created",
    "creationdate": "created",
    "date": "created",
    "encoded_date": "encoded",
    "tagged_date": "tagged",
    "com.apple.quicktime.creationdate": "created",
    "com.apple.quicktime.creation_time": "created",
}
_EXIF_PATTERN = re.compile(
    r"^(?P<year>\d{4}):(?P<month>\d{2}):(?P<day>\d{2})"
    r"[ T](?P<hour>\d{2}):(?P<minute>\d{2}):(?P<second>\d{2})$"
)
_PDF_PATTERN = re.compile(
    r"^D:(?P<year>\d{4})(?P<month>\d{2})?(?P<day>\d{2})?"
    r"(?P<hour>\d{2})?(?P<minute>\d{2})?(?P<second>\d{2})?"
    r"(?P<offset>Z|[+-]\d{2}'?\d{2}'?)?$"
)


def _precision(
    *, month: str | None, day: str | None, hour: str | None, minute: str | None, second: str | None
) -> str:
    if second is not None:
        return "second"
    if minute is not None:
        return "minute"
    if hour is not None:
        return "hour"
    if day is not None:
        return "day"
    if month is not None:
        return "month"
    return "year"


def _pdf_datetime(value: str) -> tuple[datetime, str] | None:
    match = _PDF_PATTERN.fullmatch(value)
    if match is None:
        return None
    groups = match.groupdict()
    try:
        parsed = datetime(
            int(groups["year"]),
            int(groups["month"] or 1),
            int(groups["day"] or 1),
            int(groups["hour"] or 0),
            int(groups["minute"] or 0),
            int(groups["second"] or 0),
        )
    except ValueError:
        return None

    offset = groups["offset"]
    if offset == "Z":
        parsed = parsed.replace(tzinfo=UTC)
    elif offset:
        sign = 1 if offset[0] == "+" else -1
        digits = offset[1:].replace("'", "")
        hours = int(digits[:2])
        minutes = int(digits[2:4])
        if hours > 23 or minutes > 59:
            return None
        parsed = parsed.replace(tzinfo=timezone(sign * timedelta(hours=hours, minutes=minutes)))
    return parsed, _precision(
        month=groups["month"],
        day=groups["day"],
        hour=groups["hour"],
        minute=groups["minute"],
        second=groups["second"],
    )


def _parse_timestamp(value: Any, source_field: str) -> tuple[datetime, str] | None:
    if source_field == "mtime" and isinstance(value, (int, float)) and not isinstance(value, bool):
        try:
            parsed = datetime.fromtimestamp(float(value), UTC)
            return parsed, "microsecond" if parsed.microsecond else "second"
        except (OSError, OverflowError, ValueError):
            return None
    if not isinstance(value, str):
        return None
    text = value.strip().rstrip("\x00")
    if not text:
        return None

    pdf_result = _pdf_datetime(text)
    if pdf_result is not None:
        return pdf_result

    exif_match = _EXIF_PATTERN.fullmatch(text)
    if exif_match is not None:
        try:
            return datetime(
                **{key: int(item) for key, item in exif_match.groupdict().items()}
            ), "second"
        except ValueError:
            return None

    normalized = text[:-1] + "+00:00" if text.endswith(("Z", "z")) else text
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError:
        return None
    precision = "microsecond" if parsed.microsecond else "second"
    if "T" not in text and " " not in text:
        precision = "day"
    return parsed, precision


def _normalized_timestamp(value: datetime) -> tuple[str, bool, datetime]:
    timezone_known = value.utcoffset() is not None
    if timezone_known:
        utc_value = value.astimezone(UTC)
        timestamp = utc_value.isoformat(
            timespec="microseconds" if utc_value.microsecond else "seconds"
        )
        timestamp = timestamp.replace("+00:00", "Z")
        sort_value = utc_value.replace(tzinfo=None)
    else:
        timestamp = value.isoformat(timespec="microseconds" if value.microsecond else "seconds")
        sort_value = value
    return timestamp, timezone_known, sort_value


def _video_date_values(metadata: Mapping[str, Any]) -> Iterable[tuple[str, str, Any]]:
    def walk(value: Any, prefix: str) -> Iterable[tuple[str, str, Any]]:
        if isinstance(value, Mapping):
            for key, nested in value.items():
                name = str(key)
                field = f"{prefix}.{name}" if prefix else name
                event = _VIDEO_DATE_FIELDS.get(name.casefold())
                if event is not None:
                    yield field, event, nested
                yield from walk(nested, field)
        elif isinstance(value, list):
            for index, nested in enumerate(value):
                yield from walk(nested, f"{prefix}.{index}")

    for root in ("video_tags", "video_streams"):
        if root in metadata:
            yield from walk(metadata[root], root)


def build_timeline(records: Iterable[dict[str, Any]]) -> list[TimelineEvent]:
    valid_records = require_valid_records(records)
    prepared: list[tuple[datetime, TimelineEvent]] = []
    for record in valid_records:
        path = str(record["path"])
        metadata = record["metadata"]
        sources = [
            (field, event, source_kind, metadata.get(field))
            for field, event, source_kind in _FIELD_SPECS
            if metadata.get(field) not in (None, "")
        ]
        sources.extend(
            (field, event, "video", value)
            for field, event, value in _video_date_values(metadata)
            if value not in (None, "")
        )
        for source_field, event, source_kind, source_value in sources:
            parsed = _parse_timestamp(source_value, source_field)
            if parsed is None:
                continue
            parsed_value, precision = parsed
            timestamp, timezone_known, sort_value = _normalized_timestamp(parsed_value)
            prepared.append(
                (
                    sort_value,
                    TimelineEvent(
                        timestamp=timestamp,
                        path=path,
                        event=event,
                        source_field=source_field,
                        source_kind=source_kind,
                        source_value=source_value,
                        timezone_known=timezone_known,
                        precision=precision,
                    ),
                )
            )
    prepared.sort(
        key=lambda item: (
            item[0],
            item[1].path.casefold(),
            item[1].source_field.casefold(),
        )
    )
    return [event for _, event in prepared]


def timeline_document(records: Iterable[dict[str, Any]]) -> dict[str, Any]:
    events = build_timeline(records)
    return {
        "total_events": len(events),
        "events": [asdict(event) for event in events],
    }
