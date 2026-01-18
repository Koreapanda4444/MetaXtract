from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any, Optional


@dataclass(frozen=True)
class ImageExtractResult:
    ok: bool
    data: dict[str, Any]
    error_code: Optional[str] = None


def _as_float_rational(value: Any) -> Optional[float]:
    try:
        if value is None:
            return None
        if hasattr(value, "numerator") and hasattr(value, "denominator"):
            den = float(value.denominator)
            if den == 0:
                return None
            return float(value.numerator) / den
        if isinstance(value, tuple) and len(value) == 2:
            num, den = value
            den_f = float(den)
            if den_f == 0:
                return None
            return float(num) / den_f
        if isinstance(value, (int, float)):
            return float(value)
        return float(value)
    except Exception:
        return None


def _dms_to_decimal(dms: Any, ref: Any) -> Optional[float]:
    try:
        if not isinstance(dms, (list, tuple)) or len(dms) != 3:
            return None
        deg = _as_float_rational(dms[0])
        minutes = _as_float_rational(dms[1])
        seconds = _as_float_rational(dms[2])
        if deg is None or minutes is None or seconds is None:
            return None

        dec = deg + (minutes / 60.0) + (seconds / 3600.0)
        ref_text = str(ref or "").strip().upper()
        if ref_text in {"S", "W"}:
            dec = -dec
        return dec
    except Exception:
        return None


def _parse_exif_datetime(value: Any) -> Optional[str]:
    if value is None:
        return None
    text = str(value).strip()
    if not text:
        return None
    for fmt in ("%Y:%m:%d %H:%M:%S", "%Y-%m-%d %H:%M:%S"):
        try:
            dt = datetime.strptime(text, fmt)
            return dt.isoformat()
        except Exception:
            pass

    return None


def extract_image_metadata(path: Path) -> ImageExtractResult:
    try:
                from PIL import Image, ExifTags
    except Exception:
        return ImageExtractResult(ok=False, data={}, error_code="missing_dependency")

    try:
        with Image.open(path) as img:
            data: dict[str, Any] = {
                "image": {
                    "format": img.format,
                    "width": int(getattr(img, "width", 0) or 0),
                    "height": int(getattr(img, "height", 0) or 0),
                    "mode": getattr(img, "mode", None),
                }
            }

            exif_obj = None
            try:
                exif_obj = img.getexif()
            except Exception:
                exif_obj = None

            if not exif_obj:
                return ImageExtractResult(ok=False, data=data, error_code="no_exif")
            tags = getattr(ExifTags, "TAGS", {})
            gps_tags = getattr(ExifTags, "GPSTAGS", {})

            exif_out: dict[str, Any] = {}
            gps_out: dict[str, Any] = {}

            for tag_id, value in exif_obj.items():
                name = tags.get(tag_id, str(tag_id))

                if name == "GPSInfo" and isinstance(value, dict):
                    for gps_id, gps_val in value.items():
                        gps_name = gps_tags.get(gps_id, str(gps_id))
                        gps_out[gps_name] = gps_val
                    continue

                if name in {
                    "DateTimeOriginal",
                    "DateTimeDigitized",
                    "Make",
                    "Model",
                    "Software",
                    "Orientation",
                    "LensModel",
                }:
                    exif_out[name] = value

            if exif_out:
                data["exif"] = exif_out
            if gps_out:
                data["gps"] = gps_out
            if gps_out:
                lat = _dms_to_decimal(gps_out.get("GPSLatitude"), gps_out.get("GPSLatitudeRef"))
                lon = _dms_to_decimal(gps_out.get("GPSLongitude"), gps_out.get("GPSLongitudeRef"))

                alt = _as_float_rational(gps_out.get("GPSAltitude"))
                alt_ref = gps_out.get("GPSAltitudeRef")
                try:
                    if alt is not None and str(alt_ref).strip() == "1":
                        alt = -alt
                except Exception:
                    pass

                dop = _as_float_rational(gps_out.get("GPSDOP"))

                norm_gps: dict[str, Any] = {}
                if lat is not None and lon is not None:
                    norm_gps["lat"] = float(lat)
                    norm_gps["lon"] = float(lon)
                if alt is not None:
                    norm_gps["alt_m"] = float(alt)
                if dop is not None:
                    norm_gps["dop"] = float(dop)

                if norm_gps:
                    data["gps_norm"] = norm_gps
            dt_original = _parse_exif_datetime(exif_out.get("DateTimeOriginal") if exif_out else None)
            dt_digitized = _parse_exif_datetime(exif_out.get("DateTimeDigitized") if exif_out else None)
            times: dict[str, Any] = {}
            if dt_original:
                times["DateTimeOriginal"] = dt_original
            if dt_digitized:
                times["DateTimeDigitized"] = dt_digitized
            if times:
                data["exif_times"] = times

            return ImageExtractResult(ok=True, data=data)

    except OSError:
        return ImageExtractResult(ok=False, data={}, error_code="read_error")
    except Exception:
        return ImageExtractResult(ok=False, data={}, error_code="extract_error")
