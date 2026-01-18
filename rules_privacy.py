from __future__ import annotations

from typing import Any


def _get_nested(d: dict, path: list[str]) -> Any:
    cur: Any = d
    for k in path:
        if not isinstance(cur, dict):
            return None
        cur = cur.get(k)
    return cur


def _has_nonempty_str(v: Any) -> bool:
    return isinstance(v, str) and bool(v.strip())


def _mask_text(v: Any) -> Any:
    if not _has_nonempty_str(v):
        return v
    s = str(v).strip()
    if len(s) <= 1:
        return "***"
    return s[0] + "***"


def compute_privacy_flags(record: dict) -> dict[str, bool]:
    geo = record.get("geo")
    identity = record.get("identity")
    capture = record.get("capture")

    lat = _get_nested(record, ["geo", "lat"]) if isinstance(geo, dict) else None
    lon = _get_nested(record, ["geo", "lon"]) if isinstance(geo, dict) else None

    has_gps = isinstance(lat, (int, float)) and isinstance(lon, (int, float))

    author = _get_nested(record, ["identity", "author"]) if isinstance(identity, dict) else None
    has_author = _has_nonempty_str(author)

    model = _get_nested(record, ["capture", "model"]) if isinstance(capture, dict) else None
    make = _get_nested(record, ["capture", "make"]) if isinstance(capture, dict) else None
    has_device_model = _has_nonempty_str(model) or _has_nonempty_str(make)

    software = _get_nested(record, ["capture", "software"]) if isinstance(capture, dict) else None
    has_software_trace = _has_nonempty_str(software)

    precise_time = (
        _has_nonempty_str(_get_nested(record, ["capture", "datetime_original"]))
        or _has_nonempty_str(_get_nested(record, ["meta_times", "digitized"]))
        or _has_nonempty_str(_get_nested(record, ["meta_times", "created"]))
        or _has_nonempty_str(_get_nested(record, ["meta_times", "modified"]))
    )

    return {
        "has_gps": bool(has_gps),
        "has_author": bool(has_author),
        "has_device_model": bool(has_device_model),
        "has_software_trace": bool(has_software_trace),
        "has_precise_time": bool(precise_time),
    }


def build_risk_summary(flags: dict[str, bool]) -> str:
    lines: list[str] = []

    if flags.get("has_gps"):
        lines.append("GPS 위치 정보가 포함되어 위치 노출 위험이 있습니다.")
    if flags.get("has_author"):
        lines.append("작성자/소유자 정보가 포함되어 개인 식별 가능성이 있습니다.")
    if flags.get("has_device_model") or flags.get("has_software_trace"):
        lines.append("기기/소프트웨어 흔적이 포함되어 추적 가능성이 있습니다.")
    if flags.get("has_precise_time"):
        lines.append("정밀한 시간 정보가 포함되어 활동 패턴 노출 가능성이 있습니다.")

    if not lines:
        lines.append("개인정보 위험 신호가 감지되지 않았습니다.")

    if len(lines) == 1:
        lines.append("공유/업로드 전 필요한 경우 메타데이터 제거 또는 마스킹을 권장합니다.")

    return "\n".join(lines[:4])


def redact_record_view(record: dict, flags: dict[str, bool]) -> None:
    if flags.get("has_gps"):
        geo = record.get("geo")
        if isinstance(geo, dict):
            if "lat" in geo:
                geo["lat"] = None
            if "lon" in geo:
                geo["lon"] = None
            if "alt_m" in geo:
                geo["alt_m"] = None

    if flags.get("has_author"):
        ident = record.get("identity")
        if isinstance(ident, dict) and "author" in ident:
            ident["author"] = _mask_text(ident.get("author"))


def apply_privacy_intelligence(record: dict, *, redact: bool = False) -> dict:
    signals = record.get("signals")
    if not isinstance(signals, dict):
        signals = {}
        record["signals"] = signals

    flags = compute_privacy_flags(record)
    signals["privacy_flags"] = flags
    signals["risk_summary"] = build_risk_summary(flags)

    if redact:
        redact_record_view(record, flags)

    return record
