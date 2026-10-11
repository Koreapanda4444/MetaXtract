from __future__ import annotations

from typing import Any, Dict, List, Tuple

from PIL import ExifTags, Image

from .normalize import as_float, as_int, json_safe, normalize_metadata
from ..core.files import PathLike


_GPS_TAG = 34853
_ORIENTATION_LABELS = {
    1: "normal",
    2: "mirrored_horizontal",
    3: "rotated_180",
    4: "mirrored_vertical",
    5: "mirrored_horizontal_rotated_270",
    6: "rotated_90",
    7: "mirrored_horizontal_rotated_90",
    8: "rotated_270",
}


def _rational_to_float(value: Any) -> float | None:
    result = as_float(value)
    if result is not None:
        return result
    try:
        numerator, denominator = value
    except (TypeError, ValueError):
        return None
    left = as_float(numerator)
    right = as_float(denominator)
    if left is None or right in (None, 0.0):
        return None
    return as_float(left / right)


def _dms_to_degrees(dms: Tuple[Any, Any, Any]) -> float | None:
    degrees = _rational_to_float(dms[0])
    minutes = _rational_to_float(dms[1])
    seconds = _rational_to_float(dms[2])
    if degrees is None or minutes is None or seconds is None:
        return None
    return as_float(degrees + minutes / 60.0 + seconds / 3600.0)


def _gps_reference(value: Any) -> str:
    normalized = json_safe(value)
    return str(normalized or "").strip().upper()


def _extract_gps(gps_ifd: Any) -> Dict[str, Any]:
    if not isinstance(gps_ifd, dict):
        return {}

    metadata: Dict[str, Any] = {}
    latitude = gps_ifd.get(2, gps_ifd.get("GPSLatitude"))
    longitude = gps_ifd.get(4, gps_ifd.get("GPSLongitude"))
    if (
        isinstance(latitude, (tuple, list))
        and len(latitude) == 3
        and isinstance(longitude, (tuple, list))
        and len(longitude) == 3
    ):
        latitude_degrees = _dms_to_degrees(tuple(latitude))
        longitude_degrees = _dms_to_degrees(tuple(longitude))
        if latitude_degrees is not None and longitude_degrees is not None:
            if _gps_reference(gps_ifd.get(1, gps_ifd.get("GPSLatitudeRef"))).startswith("S"):
                latitude_degrees = -latitude_degrees
            if _gps_reference(gps_ifd.get(3, gps_ifd.get("GPSLongitudeRef"))).startswith("W"):
                longitude_degrees = -longitude_degrees
            if -90 <= latitude_degrees <= 90 and -180 <= longitude_degrees <= 180:
                metadata["gps_latitude"] = latitude_degrees
                metadata["gps_longitude"] = longitude_degrees

    altitude = _rational_to_float(gps_ifd.get(6, gps_ifd.get("GPSAltitude")))
    if altitude is not None:
        altitude_ref = gps_ifd.get(5, gps_ifd.get("GPSAltitudeRef", 0))
        if isinstance(altitude_ref, bytes):
            below_sea_level = bool(altitude_ref and altitude_ref[0] == 1)
        else:
            below_sea_level = as_int(altitude_ref) == 1
        metadata["gps_altitude_m"] = -altitude if below_sea_level else altitude

    return metadata


def extract_image(path: PathLike) -> Tuple[Dict[str, Any], List[str]]:
    warnings: List[str] = []
    metadata: Dict[str, Any] = {}

    with Image.open(path) as image:
        metadata.update(
            {
                "format": image.format or "",
                "mode": image.mode,
                "width": image.width,
                "height": image.height,
                "animated": bool(getattr(image, "is_animated", False)),
                "frames": int(getattr(image, "n_frames", 1)),
            }
        )

        try:
            exif = image.getexif()
        except Exception:
            exif = None

        if exif:
            tag_map = {name: tag for tag, name in ExifTags.TAGS.items()}
            stable_tags = {
                "Make": "exif_make",
                "Model": "exif_model",
                "DateTimeOriginal": "exif_datetime_original",
                "Software": "exif_software",
                "Artist": "exif_artist",
                "LensModel": "exif_lens_model",
            }
            for tag_name, output_name in stable_tags.items():
                tag = tag_map.get(tag_name)
                if tag is not None and tag in exif:
                    metadata[output_name] = json_safe(exif.get(tag))

            orientation_tag = tag_map.get("Orientation")
            if orientation_tag is not None and orientation_tag in exif:
                orientation = as_int(exif.get(orientation_tag))
                if orientation is not None:
                    metadata["exif_orientation"] = orientation
                    metadata["exif_orientation_label"] = _ORIENTATION_LABELS.get(
                        orientation,
                        "unknown",
                    )

            try:
                gps_ifd = exif.get_ifd(_GPS_TAG)
            except (AttributeError, KeyError, TypeError, ValueError):
                gps_ifd = exif.get(_GPS_TAG)
            metadata.update(_extract_gps(gps_ifd))

    return normalize_metadata(metadata), warnings
