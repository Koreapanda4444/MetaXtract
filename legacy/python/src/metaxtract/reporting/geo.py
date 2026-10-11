from __future__ import annotations

import math
from dataclasses import asdict, dataclass
from typing import Any, Iterable, Mapping
from xml.sax.saxutils import escape

from ..core.models import require_valid_records


@dataclass(frozen=True)
class GeoLocation:
    path: str
    mime: str
    sha256: str
    latitude: float
    longitude: float
    altitude_m: float | None
    latitude_field: str
    longitude_field: str
    altitude_field: str | None


def _number(value: Any) -> float | None:
    if isinstance(value, bool):
        return None
    try:
        number = float(value)
    except (TypeError, ValueError, OverflowError):
        return None
    return number if math.isfinite(number) else None


def _mapping_item(
    mapping: Mapping[str, Any],
    names: tuple[str, ...],
) -> tuple[str, Any] | None:
    wanted = {name.casefold() for name in names}
    for key, value in mapping.items():
        if str(key).casefold() in wanted:
            return str(key), value
    return None


def _coordinate_fields(
    metadata: Mapping[str, Any],
) -> tuple[tuple[str, Any], tuple[str, Any], tuple[str, Any] | None] | None:
    latitude = _mapping_item(metadata, ("gps_latitude",))
    longitude = _mapping_item(metadata, ("gps_longitude",))
    altitude = _mapping_item(metadata, ("gps_altitude_m", "gps_altitude"))
    prefix = "metadata."

    if latitude is None or longitude is None:
        gps = _mapping_item(metadata, ("gps",))
        if gps is None or not isinstance(gps[1], Mapping):
            return None
        gps_mapping = gps[1]
        latitude = _mapping_item(gps_mapping, ("latitude", "lat", "gpslatitude"))
        longitude = _mapping_item(
            gps_mapping,
            ("longitude", "lon", "lng", "gpslongitude"),
        )
        altitude = _mapping_item(
            gps_mapping,
            ("altitude_m", "altitude", "gpsaltitude"),
        )
        prefix = f"metadata.{gps[0]}."

    if latitude is None or longitude is None:
        return None
    latitude = (prefix + latitude[0], latitude[1])
    longitude = (prefix + longitude[0], longitude[1])
    if altitude is not None:
        altitude = (prefix + altitude[0], altitude[1])
    return latitude, longitude, altitude


def extract_locations(records: Iterable[dict[str, Any]]) -> list[GeoLocation]:
    locations = []
    for record in require_valid_records(records):
        fields = _coordinate_fields(record["metadata"])
        if fields is None:
            continue
        latitude_field, longitude_field, altitude_field = fields
        latitude = _number(latitude_field[1])
        longitude = _number(longitude_field[1])
        altitude = _number(altitude_field[1]) if altitude_field is not None else None
        if latitude is None or longitude is None:
            continue
        if not -90 <= latitude <= 90 or not -180 <= longitude <= 180:
            continue
        locations.append(
            GeoLocation(
                path=str(record["path"]),
                mime=str(record["mime"]),
                sha256=str(record["sha256"]),
                latitude=latitude,
                longitude=longitude,
                altitude_m=altitude,
                latitude_field=latitude_field[0],
                longitude_field=longitude_field[0],
                altitude_field=altitude_field[0] if altitude_field is not None else None,
            )
        )
    return sorted(locations, key=lambda item: item.path.casefold())


def location_document(records: Iterable[dict[str, Any]]) -> dict[str, Any]:
    locations = extract_locations(records)
    bounds = None
    if locations:
        bounds = {
            "west": min(item.longitude for item in locations),
            "south": min(item.latitude for item in locations),
            "east": max(item.longitude for item in locations),
            "north": max(item.latitude for item in locations),
        }
    return {
        "total_locations": len(locations),
        "bounds": bounds,
        "locations": [asdict(location) for location in locations],
    }


def geojson_document(records: Iterable[dict[str, Any]]) -> dict[str, Any]:
    features = []
    for location in extract_locations(records):
        coordinates = [location.longitude, location.latitude]
        if location.altitude_m is not None:
            coordinates.append(location.altitude_m)
        features.append(
            {
                "type": "Feature",
                "id": location.path,
                "geometry": {"type": "Point", "coordinates": coordinates},
                "properties": {
                    "path": location.path,
                    "mime": location.mime,
                    "sha256": location.sha256,
                    "latitude_field": location.latitude_field,
                    "longitude_field": location.longitude_field,
                    "altitude_field": location.altitude_field,
                },
            }
        )
    return {"type": "FeatureCollection", "features": features}


def render_kml(records: Iterable[dict[str, Any]]) -> str:
    parts = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        '<kml xmlns="http://www.opengis.net/kml/2.2">',
        "<Document>",
        "<name>MetaXtract GPS locations</name>",
    ]
    for location in extract_locations(records):
        coordinate_values = [location.longitude, location.latitude]
        if location.altitude_m is not None:
            coordinate_values.append(location.altitude_m)
        coordinates = ",".join(format(value, ".15g") for value in coordinate_values)
        altitude_data = ""
        if location.altitude_field is not None:
            altitude_data = (
                '<Data name="altitude_field"><value>'
                f"{escape(location.altitude_field)}</value></Data>"
            )
        parts.extend(
            (
                "<Placemark>",
                f"<name>{escape(location.path)}</name>",
                "<ExtendedData>",
                f'<Data name="mime"><value>{escape(location.mime)}</value></Data>',
                f'<Data name="sha256"><value>{escape(location.sha256)}</value></Data>',
                f'<Data name="latitude_field"><value>{escape(location.latitude_field)}</value></Data>',
                f'<Data name="longitude_field"><value>{escape(location.longitude_field)}</value></Data>',
                altitude_data,
                "</ExtendedData>",
                f"<Point><coordinates>{coordinates}</coordinates></Point>",
                "</Placemark>",
            )
        )
    parts.extend(("</Document>", "</kml>"))
    return "\n".join(parts) + "\n"
