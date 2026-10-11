from __future__ import annotations

import math
from collections.abc import Mapping
from datetime import date, datetime, time
from decimal import Decimal
from fractions import Fraction
from typing import Any


def as_float(value: Any) -> float | None:
    try:
        result = float(value)
    except (TypeError, ValueError, ZeroDivisionError, OverflowError):
        return None
    return result if math.isfinite(result) else None


def as_int(value: Any) -> int | None:
    if isinstance(value, bool):
        return None
    try:
        return int(value)
    except (TypeError, ValueError, OverflowError):
        return None


def fraction_to_float(value: Any) -> float | None:
    if isinstance(value, str) and "/" in value:
        numerator, denominator = value.split("/", 1)
        left = as_float(numerator)
        right = as_float(denominator)
        if left is None or right in (None, 0.0):
            return None
        return as_float(left / right)
    return as_float(value)


def json_safe(value: Any) -> Any:
    if value is None or isinstance(value, (bool, int, str)):
        return value
    if isinstance(value, float):
        return value if math.isfinite(value) else None
    if isinstance(value, (datetime, date, time)):
        return value.isoformat()
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace").rstrip("\x00")
    if isinstance(value, (Decimal, Fraction)):
        return as_float(value)
    if isinstance(value, Mapping):
        return {str(key): json_safe(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [json_safe(item) for item in value]
    if isinstance(value, (set, frozenset)):
        return [json_safe(item) for item in sorted(value, key=str)]
    return str(value)


def normalize_metadata(metadata: Mapping[str, Any]) -> dict[str, Any]:
    return {str(key): json_safe(value) for key, value in metadata.items()}
