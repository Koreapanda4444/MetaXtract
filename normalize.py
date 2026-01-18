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
            out["capture"].update(capture)

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

        if isinstance(dop, (int, float)):
            geo["gps_dop"] = float(dop)
            geo["precision_flag"] = "dop"
        elif geo:
            geo["precision_flag"] = "unknown"

        if geo:
            out["geo"].update(geo)

    pdf = raw_record.get("pdf")
    if isinstance(pdf, dict):
        identity: dict[str, Any] = {}

        author = pdf.get("Author")
        if isinstance(author, str) and author.strip():
            identity["author"] = author.strip()
        if identity:
            out["identity"].update(identity)

        capture: dict[str, Any] = {}
        creator = pdf.get("Creator")
        producer = pdf.get("Producer")

        parts: list[str] = []
        if isinstance(creator, str) and creator.strip():
            parts.append(creator.strip())
        if isinstance(producer, str) and producer.strip():
            if producer.strip() not in parts:
                parts.append(producer.strip())
        if parts:
            capture["software"] = "; ".join(parts)
        if capture:
            out["capture"].update(capture)

        times = raw_record.get("pdf_times")
        if isinstance(times, dict):
            created = times.get("CreationDate")
            modified = times.get("ModDate")
            if isinstance(created, str) and created:
                out["meta_times"]["created"] = created
            if isinstance(modified, str) and modified:
                out["meta_times"]["modified"] = modified

    docx = raw_record.get("docx")
    if isinstance(docx, dict):
        identity: dict[str, Any] = {}

        creator = docx.get("creator")
        last_modified_by = docx.get("lastModifiedBy")
        title = docx.get("title")

        if isinstance(creator, str) and creator.strip():
            identity["author"] = creator.strip()
        if isinstance(last_modified_by, str) and last_modified_by.strip():
            identity["last_modified_by"] = last_modified_by.strip()
        if isinstance(title, str) and title.strip():
            identity["title"] = title.strip()
        if identity:
            out["identity"].update(identity)

        capture: dict[str, Any] = {}
        if "docx" not in str(out["capture"].get("software") or "").lower():
            capture["software"] = "docx"
        if capture:
            out["capture"].update(capture)

        times = raw_record.get("docx_times")
        if isinstance(times, dict):
            created = times.get("created")
            modified = times.get("modified")
            if isinstance(created, str) and created:
                out["meta_times"].setdefault("created", created)
            if isinstance(modified, str) and modified:
                out["meta_times"].setdefault("modified", modified)

    video = raw_record.get("video")
    if isinstance(video, dict):
        media: dict[str, Any] = {}

        duration_sec = video.get("duration_sec")
        width = video.get("width")
        height = video.get("height")
        codec = video.get("codec")
        container = video.get("container")
        container_created = video.get("container_created")

        if isinstance(duration_sec, (int, float)):
            media["duration_sec"] = float(duration_sec)
        if isinstance(width, int) and width > 0:
            media["width"] = width
        if isinstance(height, int) and height > 0:
            media["height"] = height
        if isinstance(codec, str) and codec.strip():
            media["codec"] = codec.strip()
        if isinstance(container, str) and container.strip():
            media["container"] = container.strip()
        if isinstance(container_created, str) and container_created:
            media["container_created"] = container_created

        if media:
            out["media"].update(media)

    out["raw"] = raw_record
    return out


def normalize_records(records: list[dict]) -> list[dict]:
    return [normalize_record(r) for r in records]
