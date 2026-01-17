from __future__ import annotations

from typing import Any

import schema


def normalize_record(raw_record: dict) -> dict:
    out = schema.empty_record()

    file_obj: dict[str, Any] = {}
    for k in ("path", "name", "ext", "size_bytes"):
        if k in raw_record:
            file_obj[k] = raw_record.get(k)
    out["file"] = file_obj

    os_times = raw_record.get("os_times")
    if isinstance(os_times, dict):
        out["os_times"] = os_times

    algo = raw_record.get("hash_algo")
    hx = raw_record.get("hash_hex")
    if isinstance(algo, str) and isinstance(hx, str) and algo:
        out["hashes"] = {algo: hx}

    out["raw"] = raw_record
    return out


def normalize_records(records: list[dict]) -> list[dict]:
    return [normalize_record(r) for r in records]
