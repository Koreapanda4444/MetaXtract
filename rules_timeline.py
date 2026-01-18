from __future__ import annotations

from datetime import datetime, timezone
from typing import Any


def _as_dt(value: Any) -> datetime | None:
    if value is None:
        return None
    if isinstance(value, datetime):
        if value.tzinfo is None:
            return value.replace(tzinfo=timezone.utc)
        return value
    if isinstance(value, (int, float)):
        try:
            return datetime.fromtimestamp(float(value), tz=timezone.utc)
        except Exception:
            return None
    if isinstance(value, str):
        s = value.strip()
        if not s:
            return None
        try:
            dt = datetime.fromisoformat(s)
            if dt.tzinfo is None:
                dt = dt.replace(tzinfo=timezone.utc)
            return dt
        except Exception:
            return None
    return None


def _get_nested(d: dict, path: list[str]) -> Any:
    cur: Any = d
    for k in path:
        if not isinstance(cur, dict):
            return None
        cur = cur.get(k)
    return cur


def _dt_gt(a: datetime | None, b: datetime | None, *, seconds: int) -> bool:
    if a is None or b is None:
        return False
    try:
        return (a - b).total_seconds() > float(seconds)
    except Exception:
        return False


def _abs_dt_diff_gt(a: datetime | None, b: datetime | None, *, seconds: int) -> bool:
    if a is None or b is None:
        return False
    try:
        return abs((a - b).total_seconds()) > float(seconds)
    except Exception:
        return False


def compute_timeline_flags(record: dict) -> dict[str, Any]:
    os_mtime = _as_dt(_get_nested(record, ["os_times", "mtime"]))
    os_ctime = _as_dt(_get_nested(record, ["os_times", "ctime"]))

    meta_created = _as_dt(_get_nested(record, ["meta_times", "created"]))
    meta_modified = _as_dt(_get_nested(record, ["meta_times", "modified"]))
    meta_digitized = _as_dt(_get_nested(record, ["meta_times", "digitized"]))

    tol_future_sec = 5 * 60
    tol_flip_sec = 5
    tol_digitized_created_gap_sec = 180 * 24 * 3600

    reasons: list[str] = []

    if _dt_gt(meta_created, os_mtime, seconds=tol_future_sec):
        reasons.append("meta_created_future_than_os_mtime")
    if _dt_gt(meta_modified, os_mtime, seconds=tol_future_sec):
        reasons.append("meta_modified_future_than_os_mtime")
    if _dt_gt(meta_digitized, os_mtime, seconds=tol_future_sec):
        reasons.append("meta_digitized_future_than_os_mtime")

    if _dt_gt(meta_created, meta_modified, seconds=tol_flip_sec):
        reasons.append("meta_created_after_meta_modified")

    if _dt_gt(os_ctime, os_mtime, seconds=tol_flip_sec):
        reasons.append("os_ctime_after_os_mtime")

    if _abs_dt_diff_gt(meta_digitized, meta_created, seconds=tol_digitized_created_gap_sec):
        reasons.append("meta_digitized_created_gap")

    mismatch = bool(reasons)

    explain_parts: list[str] = []
    if "meta_created_after_meta_modified" in reasons or "os_ctime_after_os_mtime" in reasons:
        explain_parts.append("시간 역전(생성/수정 순서) 징후")
    if any(r.endswith("future_than_os_mtime") for r in reasons):
        explain_parts.append("메타 시간이 OS 수정시간보다 미래")
    if "meta_digitized_created_gap" in reasons:
        explain_parts.append("digitized/created 시간 불일치")

    short_explain = ", ".join(explain_parts) if explain_parts else "정상"

    return {
        "time_mismatch": mismatch,
        "reason_codes": reasons,
        "short_explain": short_explain,
    }


def apply_timeline_checks(record: dict) -> dict:
    signals = record.get("signals")
    if not isinstance(signals, dict):
        signals = {}
        record["signals"] = signals

    signals["timeline_flags"] = compute_timeline_flags(record)
    return record
