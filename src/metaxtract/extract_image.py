from __future__ import annotations

import math
from typing import Any, Dict, List, Tuple

from PIL import Image, ExifTags

from .utils import PathLike


_GPS_TAG = 34853  # GPSInfo


def _rational_to_float(v: Any) -> float:
    # v can be IFDRational, Fraction-like, or tuple(n, d)
    try:
        return float(v)
    except Exception:
        try:
            n, d = v
            return float(n) / float(d)
        except Exception:
            return 0.0


def _dms_to_deg(dms: Tuple[Any, Any, Any]) -> float:
    deg = _rational_to_float(dms[0])
    minutes = _rational_to_float(dms[1])
    sec = _rational_to_float(dms[2])
    return deg + (minutes / 60.0) + (sec / 3600.0)


def _extract_gps(gps_ifd: Any) -> Dict[str, Any]:
    out: Dict[str, Any] = {}
    if not isinstance(gps_ifd, dict):
        return out

    lat_ref = gps_ifd.get(1, gps_ifd.get("GPSLatitudeRef"))
    lat = gps_ifd.get(2, gps_ifd.get("GPSLatitude"))
    lon_ref = gps_ifd.get(3, gps_ifd.get("GPSLongitudeRef"))
    lon = gps_ifd.get(4, gps_ifd.get("GPSLongitude"))

    if isinstance(lat_ref, bytes):
        lat_ref = lat_ref.decode("ascii", errors="ignore")
    if isinstance(lon_ref, bytes):
        lon_ref = lon_ref.decode("ascii", errors="ignore")

    if (
        isinstance(lat, (tuple, list))
        and len(lat) == 3
        and isinstance(lon, (tuple, list))
        and len(lon) == 3
    ):
        lat_deg = _dms_to_deg((lat[0], lat[1], lat[2]))
        lon_deg = _dms_to_deg((lon[0], lon[1], lon[2]))
        if str(lat_ref).upper().startswith("S"):
            lat_deg = -lat_deg
        if str(lon_ref).upper().startswith("W"):
            lon_deg = -lon_deg
        if (
            math.isfinite(lat_deg)
            and math.isfinite(lon_deg)
            and -90 <= lat_deg <= 90
            and -180 <= lon_deg <= 180
        ):
            out["gps_latitude"] = lat_deg
            out["gps_longitude"] = lon_deg
    return out


def extract_image(path: PathLike) -> Tuple[Dict[str, Any], List[str]]:
    warnings: List[str] = []
    md: Dict[str, Any] = {}

    with Image.open(path) as im:
        md["format"] = im.format
        md["mode"] = im.mode
        md["width"] = im.width
        md["height"] = im.height

        try:
            exif = im.getexif()
        except Exception:
            exif = None

        if exif:
            # Basic EXIF: map a few common tags for stability.
            tag_map = {v: k for k, v in ExifTags.TAGS.items()}
            make_tag = tag_map.get("Make")
            model_tag = tag_map.get("Model")
            dt_tag = tag_map.get("DateTimeOriginal")

            if make_tag and make_tag in exif:
                md["exif_make"] = str(exif.get(make_tag))
            if model_tag and model_tag in exif:
                md["exif_model"] = str(exif.get(model_tag))
            if dt_tag and dt_tag in exif:
                md["exif_datetime_original"] = str(exif.get(dt_tag))

            try:
                gps_ifd = exif.get_ifd(_GPS_TAG)
            except (AttributeError, KeyError, TypeError, ValueError):
                gps_ifd = exif.get(_GPS_TAG)
            md.update(_extract_gps(gps_ifd))

    return md, warnings
