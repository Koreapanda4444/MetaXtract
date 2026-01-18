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

    exif = raw_record.get("exif")
    if isinstance(exif, dict):
        capture: dict[str, Any] = {}

        make = exif.get("Make")
        model = exif.get("Model")
        software = exif.get("Software")

        if isinstance(make, str) and make.strip():
            capture["make"] = make.strip()
        if isinstance(model, str) and model.strip():
            capture["model"] = model.strip()
        if isinstance(software, str) and software.strip():
            capture["software"] = software.strip()

        times = raw_record.get("exif_times")
        if isinstance(times, dict):
            dto = times.get("DateTimeOriginal")
            dtd = times.get("DateTimeDigitized")
            if isinstance(dto, str) and dto:
                capture["datetime_original"] = dto
            if isinstance(dtd, str) and dtd:
                out["meta_times"]["digitized"] = dtd

        if capture:
            out["capture"] = capture

    gps_norm = raw_record.get("gps_norm")
    if isinstance(gps_norm, dict):
        geo: dict[str, Any] = {}

        lat = gps_norm.get("lat")
        lon = gps_norm.get("lon")
        alt_m = gps_norm.get("alt_m")
        dop = gps_norm.get("dop")

        if isinstance(lat, (int, float)):
            geo["lat"] = float(lat)
        if isinstance(lon, (int, float)):
            geo["lon"] = float(lon)
        if isinstance(alt_m, (int, float)):
            geo["alt_m"] = float(alt_m)

        # Precision flag: DOP presence is a useful, common indicator
        if isinstance(dop, (int, float)):
            geo["gps_dop"] = float(dop)
            geo["precision_flag"] = "dop"
        elif geo:
            geo["precision_flag"] = "unknown"

        if geo:
            out["geo"] = geo

    out["raw"] = raw_record
    return out


def normalize_records(records: list[dict]) -> list[dict]:
    return [normalize_record(r) for r in records]
