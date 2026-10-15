use crate::{AnalysisLimits, check_record, invalid, schema::*, timestamp};
use std::{collections::BTreeMap, io};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct NormalizationIssue {
    pub observation: ObservationId,
    pub reason: String,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EncodingInfo {
    pub observation: ObservationId,
    pub label: Option<String>,
    pub lossless: bool,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Normalization {
    pub claims: Vec<NormalizedClaim>,
    pub issues: Vec<NormalizationIssue>,
    pub encodings: Vec<EncodingInfo>,
}

pub fn identity_comparison_key(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
pub fn canonical_encoding(value: &str) -> String {
    let s = value.trim().to_ascii_lowercase().replace('_', "-");
    match s.as_str() {
        "utf8" | "utf-8" => "UTF-8".into(),
        "utf16" | "utf-16" => "UTF-16".into(),
        "utf16le" | "utf-16le" => "UTF-16LE".into(),
        "utf16be" | "utf-16be" => "UTF-16BE".into(),
        "ascii" | "us-ascii" => "US-ASCII".into(),
        "latin1" | "latin-1" | "iso-8859-1" => "ISO-8859-1".into(),
        _ => s,
    }
}
pub(crate) fn raw(o: &MetadataObservation) -> &RawValue {
    o.interpreted_value.as_ref().unwrap_or(&o.raw_value)
}
pub(crate) fn raw_text(v: &RawValue) -> Option<&str> {
    if let RawValue::Text(v) = v {
        Some(&v.text)
    } else {
        None
    }
}
pub(crate) fn number(v: &RawValue) -> Option<f64> {
    let n = match v {
        RawValue::Signed(v) => *v as f64,
        RawValue::Unsigned(v) => *v as f64,
        RawValue::Float(v) => *v,
        RawValue::Rational(v) if v.denominator != 0 => v.numerator as f64 / v.denominator as f64,
        RawValue::Text(v) => v.text.trim().parse().ok()?,
        _ => return None,
    };
    n.is_finite().then_some(n)
}
fn integer(v: &RawValue) -> Option<i128> {
    match v {
        RawValue::Signed(v) => Some(i128::from(*v)),
        RawValue::Unsigned(v) => Some(i128::from(*v)),
        RawValue::Text(v) => v.text.trim().parse().ok(),
        _ => None,
    }
}
pub(crate) fn field(o: &MetadataObservation) -> String {
    if o.tag.namespace.ends_with(".gps") {
        if let TagCode::Numeric(code) = o.tag.code {
            if let Some(name) = match code {
                1 => Some("latitude_ref"),
                2 => Some("latitude"),
                3 => Some("longitude_ref"),
                4 => Some("longitude"),
                5 => Some("altitude_ref"),
                6 => Some("altitude"),
                _ => None,
            } {
                return name.into();
            }
        }
    }
    o.tag
        .name
        .as_deref()
        .or_else(|| {
            if let TagCode::Text(v) = &o.tag.code {
                Some(v)
            } else {
                None
            }
        })
        .unwrap_or("")
        .rsplit('}')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}
pub(crate) fn scope(o: &MetadataObservation) -> String {
    let mut s = String::new();
    if let Some(stream) = &o.provenance.stream_id {
        s.push_str("streams.");
        // Hex is injective: punctuation and case cannot merge separate streams.
        s.push_str(&crate::hex(stream.as_bytes()));
        s.push('.');
    }
    for part in o.tag.namespace.split('.') {
        if part
            .strip_prefix("item")
            .or_else(|| part.strip_prefix("page"))
            .or_else(|| part.strip_prefix("frame"))
            .is_some_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
        {
            s.push_str(part);
            s.push('.');
        }
    }
    s
}
fn claim(
    id: u64,
    key: String,
    value: NormalizedValue,
    observations: Vec<&MetadataObservation>,
) -> io::Result<NormalizedClaim> {
    let mut parsers: Vec<_> = observations
        .iter()
        .map(|o| o.provenance.parser.id.clone())
        .collect();
    parsers.sort();
    parsers.dedup();
    Ok(NormalizedClaim {
        id: ClaimId::new(id),
        key: CanonicalKey::parse(key).map_err(io::Error::other)?,
        value,
        supporting_observations: observations.iter().map(|o| o.id).collect(),
        contradicting_observations: vec![],
        supporting_parsers: parsers,
        dissenting_parsers: vec![],
        confidence: Confidence::new(850, vec![ConfidenceBasis::DirectTag])
            .ok_or_else(|| invalid("invalid confidence"))?,
        agreement: AgreementLevel::SingleParser,
        resolution: ClaimResolution::Unresolved,
    })
}

/// Derive additional claims without altering raw observations or existing claims.
pub fn normalize(record: &ExtractionRecord, limits: AnalysisLimits) -> io::Result<Normalization> {
    check_record(record, limits)?;
    let mut out = Normalization {
        claims: vec![],
        issues: vec![],
        encodings: vec![],
    };
    let mut next = record
        .claims
        .iter()
        .map(|c| c.id.get())
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| invalid("claim ID exhausted"))?;
    let mut gps: BTreeMap<(String, Option<String>), Vec<&MetadataObservation>> = BTreeMap::new();
    let mut work = 0usize;
    for o in &record.observations {
        work = work
            .saturating_add(record.observations.len())
            .saturating_add(record.claims.len());
        if work > limits.max_comparisons {
            return Err(invalid("normalization comparison budget exceeded"));
        }
        if let RawValue::Text(t) = raw(o) {
            out.encodings.push(EncodingInfo {
                observation: o.id,
                label: t.original_encoding.as_deref().map(canonical_encoding),
                lossless: t.lossless,
            });
            if !t.lossless {
                out.issues.push(NormalizationIssue {
                    observation: o.id,
                    reason: "text decoding was lossy; original bytes remain in the observation"
                        .into(),
                });
            }
        }
        let name = field(o);
        let prefix = scope(o);
        if name.contains("latitude")
            || name.contains("longitude")
            || name.contains("altitude")
            || o.tag.namespace.ends_with(".gps")
        {
            gps.entry((o.tag.namespace.clone(), o.provenance.stream_id.clone()))
                .or_default()
                .push(o);
        }
        let role = match name.as_str() {
            "created" | "creationdate" | "creation_date" | "creation_time" | "created_filetime" => {
                Some("created")
            }
            "modified" | "moddate" | "modification_date" | "timestamp" | "last_saved_filetime" => {
                Some("modified")
            }
            "datetime_original" | "datetimeoriginal" => Some("captured"),
            "datetime_digitized" | "datetimedigitized" => Some("digitized"),
            "lastprinted" | "last_printed" => Some("printed"),
            _ => None,
        };
        let candidate = if let Some(role) = role {
            let value = if name.ends_with("filetime") {
                integer(raw(o))
                    .and_then(|v| v.checked_sub(116_444_736_000_000_000))
                    .and_then(|v| v.checked_mul(100))
                    .and_then(timestamp::from_unix_nanoseconds)
            } else {
                let offset_name = if role == "captured" {
                    "offset_time_original"
                } else if role == "digitized" {
                    "offset_time_digitized"
                } else {
                    "offset_time"
                };
                let offsets: Vec<_> = record
                    .observations
                    .iter()
                    .filter(|p| {
                        p.tag.namespace == o.tag.namespace
                            && p.provenance.stream_id == o.provenance.stream_id
                            && field(p) == offset_name
                    })
                    .collect();
                let zone = if offsets.len() == 1 {
                    raw_text(raw(offsets[0]))
                } else {
                    None
                };
                raw_text(raw(o)).and_then(|v| timestamp::parse_timestamp(v, zone))
            };
            if value.is_none() {
                out.issues.push(NormalizationIssue {
                    observation: o.id,
                    reason: "unrecognized or invalid timestamp; raw value retained".into(),
                });
            }
            value.map(|v| {
                (
                    format!("{prefix}metadata.{role}"),
                    NormalizedValue::Timestamp(v),
                )
            })
        } else {
            let identity = match name.as_str() {
                "author" | "creator" | "artist" => Some("author"),
                "lastmodifiedby" | "last_modified_by" => Some("last_editor"),
                "software" | "producer" | "application" => Some("software"),
                "body_serial_number" => Some("device_serial"),
                "lens_serial_number" => Some("lens_serial"),
                "model" => Some("device_model"),
                "make" => Some("device_make"),
                "title" => Some("title"),
                _ => None,
            };
            if let Some(key) = identity {
                raw_text(raw(o)).map(|v| {
                    (
                        format!("{prefix}metadata.{key}"),
                        NormalizedValue::Text(v.trim().trim_end_matches('\0').to_owned()),
                    )
                })
            } else if let Some(multiplier) = match name.as_str() {
                "duration_ms" => Some(1_000_000i128),
                "duration_us" => Some(1000),
                "duration_ns" => Some(1),
                _ => None,
            } {
                integer(raw(o))
                    .and_then(|v| v.checked_mul(multiplier))
                    .filter(|v| *v >= 0)
                    .map(|v| {
                        (
                            format!("{prefix}media.duration"),
                            NormalizedValue::DurationNanoseconds(v),
                        )
                    })
            } else if name == "bitrate_kbps" {
                integer(raw(o))
                    .and_then(|v| v.checked_mul(1000))
                    .and_then(|v| u64::try_from(v).ok())
                    .map(|v| {
                        (
                            format!("{prefix}media.bitrate_bps"),
                            NormalizedValue::Unsigned(v),
                        )
                    })
            } else {
                None
            }
        };
        if let Some((key, value)) = candidate {
            let new = claim(next, key, value, vec![o])?;
            if !record.claims.iter().any(|c| {
                c.key == new.key
                    && c.value == new.value
                    && c.supporting_observations == new.supporting_observations
            }) {
                out.claims.push(new);
                next = next
                    .checked_add(1)
                    .ok_or_else(|| invalid("claim ID exhausted"))?;
            }
        }
    }
    for observations in gps.values() {
        let find = |names: &[&str]| {
            observations
                .iter()
                .copied()
                .find(|o| names.contains(&field(o).as_str()))
        };
        let lat = find(&["gps_latitude", "latitude"]);
        let lon = find(&["gps_longitude", "longitude"]);
        if let (Some(lat), Some(lon)) = (lat, lon) {
            let coordinate =
                |o: &MetadataObservation, reference: Option<&MetadataObservation>| -> Option<f64> {
                    let mut v = if let RawValue::Sequence(values) = raw(o) {
                        if values.len() != 3 {
                            return None;
                        }
                        let d = number(&values[0])?;
                        let m = number(&values[1])?;
                        let s = number(&values[2])?;
                        if d < 0.0 || !(0.0..60.0).contains(&m) || !(0.0..60.0).contains(&s) {
                            return None;
                        }
                        d + m / 60.0 + s / 3600.0
                    } else {
                        number(raw(o))?
                    };
                    if let Some(reference) = reference {
                        match raw_text(raw(reference))?
                            .trim_end_matches('\0')
                            .to_ascii_uppercase()
                            .as_str()
                        {
                            "S" | "W" => v = -v.abs(),
                            "N" | "E" => v = v.abs(),
                            _ => return None,
                        }
                    }
                    Some(v)
                };
            let lat_ref = find(&["gps_latitude_ref", "latitude_ref"]);
            let lon_ref = find(&["gps_longitude_ref", "longitude_ref"]);
            let altitude = find(&["gps_altitude", "altitude"]);
            let altitude_ref = find(&["gps_altitude_ref", "altitude_ref"]);
            let point = coordinate(lat, lat_ref).zip(coordinate(lon, lon_ref)).map(
                |(latitude, longitude)| GeoPoint {
                    latitude,
                    longitude,
                    altitude_meters: altitude.and_then(|o| number(raw(o))).map(|v| {
                        if altitude_ref.and_then(|o| number(raw(o))) == Some(1.0) {
                            -v.abs()
                        } else {
                            v
                        }
                    }),
                    horizontal_accuracy_meters: None,
                },
            );
            if let Some(point) = point.filter(|p| p.is_valid()) {
                let mut support = vec![lat, lon];
                support.extend(
                    [lat_ref, lon_ref, altitude, altitude_ref]
                        .into_iter()
                        .flatten(),
                );
                let new = claim(
                    next,
                    format!("{}location.gps", scope(lat)),
                    NormalizedValue::GeoPoint(point),
                    support,
                )?;
                if !record.claims.iter().any(|c| {
                    c.key == new.key
                        && c.value == new.value
                        && c.supporting_observations == new.supporting_observations
                }) {
                    out.claims.push(new);
                    next = next
                        .checked_add(1)
                        .ok_or_else(|| invalid("claim ID exhausted"))?;
                }
            } else {
                out.issues.push(NormalizationIssue {
                    observation: lat.id,
                    reason: "invalid GPS coordinates or reference".into(),
                });
            }
        }
    }
    if out
        .claims
        .len()
        .saturating_add(out.issues.len())
        .saturating_add(out.encodings.len())
        > limits.max_results
    {
        return Err(invalid("normalization result budget exceeded"));
    }
    Ok(out)
}
