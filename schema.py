from __future__ import annotations

TOP_LEVEL_KEYS = (
    "file",
    "os_times",
    "hashes",
    "meta_times",
    "identity",
    "capture",
    "geo",
    "media",
    "signals",
    "raw",
)


def empty_record() -> dict:
    return {
        "file": {},
        "os_times": {},
        "hashes": {},
        "meta_times": {},
        "identity": {},
        "capture": {},
        "geo": {},
        "media": {},
        "signals": {
            "privacy_flags": {},
            "risk_summary": "",
            "timeline_flags": {"time_mismatch": False, "reason_codes": [], "short_explain": ""},
        },
        "raw": {},
    }


def has_top_level_keys(record: dict) -> bool:
    try:
        return set(record.keys()) == set(TOP_LEVEL_KEYS)
    except Exception:
        return False
