from __future__ import annotations

import json
import re
import shlex
from dataclasses import dataclass
from typing import Any, Iterable, Mapping


PREDICATE_PATTERN = re.compile(
    r"^(?P<field>[A-Za-z_][A-Za-z0-9_.-]*)(?P<operator>!=|>=|<=|=|>|<|:)(?P<value>.+)$"
)


class QuerySyntaxError(ValueError):
    pass


@dataclass(frozen=True)
class QueryPredicate:
    field: str | None
    operator: str
    value: str


def parse_query(query: str) -> list[QueryPredicate]:
    try:
        tokens = shlex.split(query)
    except ValueError as exc:
        raise QuerySyntaxError(str(exc)) from exc

    predicates = []
    for token in tokens:
        match = PREDICATE_PATTERN.fullmatch(token)
        if match is None:
            predicates.append(QueryPredicate(None, ":", token))
            continue
        predicates.append(
            QueryPredicate(
                match.group("field").casefold(),
                match.group("operator"),
                match.group("value"),
            )
        )
    return predicates


def _record_status(record: Mapping[str, Any]) -> str:
    if record.get("errors"):
        return "Error"
    if record.get("warnings"):
        return "Warning"
    return "OK"


def _annotation_values(annotation: Any) -> tuple[str, tuple[str, ...]]:
    if annotation is None:
        return "", ()
    if isinstance(annotation, Mapping):
        note = annotation.get("note", "")
        tags = annotation.get("tags", ())
    else:
        note = getattr(annotation, "note", "")
        tags = getattr(annotation, "tags", ())
    return str(note or ""), tuple(str(tag) for tag in (tags or ()))


def _mapping_value(mapping: Mapping[str, Any], key: str) -> Any:
    if key in mapping:
        return mapping[key]
    folded = key.casefold()
    for current_key, value in mapping.items():
        if str(current_key).casefold() == folded:
            return value
    return None


def _field_value(record: Mapping[str, Any], field: str, annotation: Any) -> Any:
    aliases = {
        "hash": "sha256",
        "size": "size_bytes",
        "warning": "warnings",
        "error": "errors",
        "meta": "metadata",
    }
    note, tags = _annotation_values(annotation)
    if field == "status":
        return _record_status(record)
    if field == "tag":
        return tags
    if field == "note":
        return note

    parts = field.split(".")
    parts[0] = aliases.get(parts[0], parts[0])
    current: Any = record
    for part in parts:
        if not isinstance(current, Mapping):
            return None
        current = _mapping_value(current, part)
    return current


def _scalar_values(value: Any) -> list[Any]:
    if isinstance(value, Mapping):
        values = []
        for key, nested in value.items():
            values.append(key)
            values.extend(_scalar_values(nested))
        return values
    if isinstance(value, (list, tuple, set)):
        values = []
        for nested in value:
            values.extend(_scalar_values(nested))
        return values
    return [value]


def _equal(candidate: Any, expected: str) -> bool:
    if candidate is None:
        return expected.casefold() in {"null", "none"}
    if isinstance(candidate, bool):
        return expected.casefold() == str(candidate).casefold()
    if isinstance(candidate, (int, float)) and not isinstance(candidate, bool):
        try:
            return float(candidate) == float(expected)
        except ValueError:
            return False
    return str(candidate).casefold() == expected.casefold()


def _compare(candidate: Any, operator: str, expected: str) -> bool:
    values = _scalar_values(candidate)
    if operator == ":":
        needle = expected.casefold()
        return any(needle in str(value).casefold() for value in values)
    if operator == "=":
        return any(_equal(value, expected) for value in values)
    if operator == "!=":
        return not any(_equal(value, expected) for value in values)

    try:
        expected_number = float(expected)
    except ValueError as exc:
        raise QuerySyntaxError(
            f"operator {operator} requires a numeric value: {expected}"
        ) from exc
    comparisons = {
        ">": lambda value: value > expected_number,
        ">=": lambda value: value >= expected_number,
        "<": lambda value: value < expected_number,
        "<=": lambda value: value <= expected_number,
    }
    for value in values:
        if isinstance(value, bool):
            continue
        try:
            number = float(value)
        except (TypeError, ValueError):
            continue
        if comparisons[operator](number):
            return True
    return False


def _searchable_text(record: Mapping[str, Any], annotation: Any) -> str:
    note, tags = _annotation_values(annotation)
    return " ".join(
        (
            json.dumps(record, ensure_ascii=False, sort_keys=True),
            note,
            " ".join(tags),
            _record_status(record),
        )
    ).casefold()


def matches_query(
    record: Mapping[str, Any],
    predicates: Iterable[QueryPredicate],
    *,
    annotation: Any = None,
) -> bool:
    searchable = None
    for predicate in predicates:
        if predicate.field is None:
            if searchable is None:
                searchable = _searchable_text(record, annotation)
            if predicate.value.casefold() not in searchable:
                return False
            continue
        candidate = _field_value(record, predicate.field, annotation)
        if not _compare(candidate, predicate.operator, predicate.value):
            return False
    return True


def filter_query(
    records: Iterable[dict[str, Any]],
    query: str,
    *,
    status: str = "All",
    annotations: Mapping[str, Any] | None = None,
) -> list[dict[str, Any]]:
    predicates = parse_query(query)
    annotation_index = annotations or {}
    result = []
    for record in records:
        if status != "All" and _record_status(record) != status:
            continue
        annotation = annotation_index.get(str(record.get("path") or ""))
        if matches_query(record, predicates, annotation=annotation):
            result.append(record)
    return result
