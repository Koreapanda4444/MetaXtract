use crate::{AnalysisLimits, check_record, invalid, schema::*};
use std::{collections::BTreeMap, io};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct GeoSample {
    pub path: EvidencePath,
    pub claim: ClaimId,
    pub point: GeoPoint,
    pub observations: Vec<ObservationId>,
    pub timestamp: Option<TimestampValue>,
    pub ambiguous_time: bool,
}
pub fn locations(
    records: &[ExtractionRecord],
    limits: AnalysisLimits,
) -> io::Result<Vec<GeoSample>> {
    if records.len() > limits.max_records {
        return Err(invalid("location record budget exceeded"));
    }
    let mut out = Vec::new();
    let mut comparisons = 0usize;
    for record in records {
        check_record(record, limits)?;
        for claim in &record.claims {
            if claim.resolution == ClaimResolution::Rejected {
                continue;
            }
            if let NormalizedValue::GeoPoint(point) = claim.value {
                if !point.is_valid() {
                    return Err(invalid("invalid geographic coordinate"));
                }
                comparisons = comparisons.saturating_add(record.claims.len());
                if comparisons > limits.max_comparisons {
                    return Err(invalid("location time association budget exceeded"));
                }
                let prefix = claim.key.as_str().strip_suffix("location.gps");
                let times: Vec<_> = record
                    .claims
                    .iter()
                    .filter(|c| {
                        c.resolution != ClaimResolution::Rejected
                            && prefix
                                .is_some_and(|p| c.key.as_str() == format!("{p}metadata.captured"))
                    })
                    .filter_map(|c| {
                        if let NormalizedValue::Timestamp(t) = &c.value {
                            Some(t)
                        } else {
                            None
                        }
                    })
                    .collect();
                let time = times
                    .first()
                    .filter(|t| times.iter().all(|other| other == *t))
                    .copied()
                    .cloned();
                out.push(GeoSample {
                    path: record.source.path.clone(),
                    claim: claim.id,
                    point,
                    observations: claim.supporting_observations.clone(),
                    timestamp: time,
                    ambiguous_time: times.len() > 1
                        && times.iter().any(|t| Some(*t) != times.first().copied()),
                });
                if out.len() > limits.max_results {
                    return Err(invalid("location result budget exceeded"));
                }
            }
        }
    }
    out.sort_by_key(|p| (p.path.to_string(), p.claim));
    Ok(out)
}
pub fn distance_meters(a: GeoPoint, b: GeoPoint) -> io::Result<f64> {
    if !a.is_valid() || !b.is_valid() {
        return Err(invalid("invalid distance coordinates"));
    }
    let lat = (b.latitude - a.latitude).to_radians();
    let lon = (b.longitude - a.longitude).to_radians();
    let v = (lat / 2.0).sin().powi(2)
        + a.latitude.to_radians().cos() * b.latitude.to_radians().cos() * (lon / 2.0).sin().powi(2);
    Ok(2.0 * 6_371_008.8 * v.clamp(0.0, 1.0).sqrt().asin())
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct GeoCluster {
    pub members: Vec<usize>,
    pub centroid: Option<GeoPoint>,
}
fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}
/// Single-link radius components, not a density estimate or proof of a common source.
pub fn clusters(
    samples: &[GeoSample],
    radius_meters: f64,
    min_members: usize,
    limits: AnalysisLimits,
) -> io::Result<Vec<GeoCluster>> {
    if !radius_meters.is_finite()
        || radius_meters < 0.0
        || min_members == 0
        || samples.len() > limits.max_records
    {
        return Err(invalid("invalid geographic cluster parameters"));
    }
    let mut parent: Vec<_> = (0..samples.len()).collect();
    let mut comparisons = 0usize;
    for i in 0..samples.len() {
        for j in i + 1..samples.len() {
            comparisons += 1;
            if comparisons > limits.max_comparisons {
                return Err(invalid("geographic comparison budget exceeded"));
            }
            if distance_meters(samples[i].point, samples[j].point)? <= radius_meters {
                let a = root(&mut parent, i);
                let b = root(&mut parent, j);
                parent[b] = a;
            }
        }
    }
    let mut grouped: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..samples.len() {
        grouped.entry(root(&mut parent, i)).or_default().push(i);
    }
    let mut out = Vec::new();
    for members in grouped.into_values().filter(|v| v.len() >= min_members) {
        let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
        for i in &members {
            let p = samples[*i].point;
            if !p.is_valid() {
                return Err(invalid("invalid cluster coordinate"));
            }
            let lat = p.latitude.to_radians();
            let lon = p.longitude.to_radians();
            x += lat.cos() * lon.cos();
            y += lat.cos() * lon.sin();
            z += lat.sin();
        }
        let centroid = if x * x + y * y + z * z < 1e-20 {
            None
        } else {
            Some(GeoPoint {
                latitude: z.atan2(x.hypot(y)).to_degrees(),
                longitude: y.atan2(x).to_degrees(),
                altitude_meters: None,
                horizontal_accuracy_meters: None,
            })
        };
        out.push(GeoCluster { members, centroid });
    }
    if out.len() > limits.max_results {
        return Err(invalid("cluster result budget exceeded"));
    }
    Ok(out)
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Track {
    pub group: String,
    pub samples: Vec<usize>,
    pub distance_meters: f64,
    pub speed_outliers: Vec<(usize, usize)>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Tracks {
    pub segments: Vec<Track>,
    pub unresolved: Vec<usize>,
}
pub fn tracks(
    samples: &[GeoSample],
    groups: &BTreeMap<String, String>,
    max_gap_seconds: u64,
    max_speed_mps: f64,
    limits: AnalysisLimits,
) -> io::Result<Tracks> {
    if samples.len() > limits.max_records
        || max_gap_seconds == 0
        || !max_speed_mps.is_finite()
        || max_speed_mps <= 0.0
    {
        return Err(invalid("invalid track parameters"));
    }
    let mut out = Tracks {
        segments: vec![],
        unresolved: vec![],
    };
    let mut grouped: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, p) in samples.iter().enumerate() {
        if !p.point.is_valid() {
            return Err(invalid("invalid track point"));
        }
        if p.ambiguous_time || p.timestamp.as_ref().and_then(|t| t.unix_seconds).is_none() {
            out.unresolved.push(i);
            continue;
        }
        let path = p.path.to_string();
        grouped
            .entry(groups.get(&path).cloned().unwrap_or(path))
            .or_default()
            .push(i);
    }
    let instant = |i: usize| -> i128 {
        let t = samples[i].timestamp.as_ref().expect("filtered time");
        i128::from(t.unix_seconds.expect("filtered UTC")) * 1_000_000_000
            + i128::from(t.nanosecond.unwrap_or(0))
    };
    for (group, mut indices) in grouped {
        indices.sort_by_key(|i| (instant(*i), *i));
        let mut current = Track {
            group: group.clone(),
            samples: vec![],
            distance_meters: 0.0,
            speed_outliers: vec![],
        };
        for i in indices {
            if let Some(previous) = current.samples.last().copied() {
                let dt = instant(i) - instant(previous);
                let distance = distance_meters(samples[previous].point, samples[i].point)?;
                if dt > i128::from(max_gap_seconds) * 1_000_000_000 {
                    out.segments.push(current);
                    current = Track {
                        group: group.clone(),
                        samples: vec![],
                        distance_meters: 0.0,
                        speed_outliers: vec![],
                    };
                } else {
                    current.distance_meters += distance;
                    if (dt == 0 && distance > 0.0)
                        || (dt > 0 && distance / (dt as f64 / 1e9) > max_speed_mps)
                    {
                        current.speed_outliers.push((previous, i));
                    }
                }
            }
            current.samples.push(i);
        }
        if !current.samples.is_empty() {
            out.segments.push(current);
        }
    }
    if out.segments.len().saturating_add(out.unresolved.len()) > limits.max_results {
        return Err(invalid("track result budget exceeded"));
    }
    Ok(out)
}
pub fn geojson(samples: &[GeoSample], limits: AnalysisLimits) -> io::Result<serde_json::Value> {
    if samples.len() > limits.max_results {
        return Err(invalid("geographic export limit exceeded"));
    }
    let mut features = Vec::new();
    for p in samples {
        if !p.point.is_valid() {
            return Err(invalid("invalid export coordinate"));
        }
        let mut coordinates = vec![p.point.longitude, p.point.latitude];
        if let Some(a) = p.point.altitude_meters {
            coordinates.push(a);
        }
        features.push(serde_json::json!({"type":"Feature","geometry":{"type":"Point","coordinates":coordinates},"properties":{"path":p.path.to_string(),"claim":p.claim,"observations":p.observations,"timestamp":p.timestamp,"ambiguous_time":p.ambiguous_time,"horizontal_accuracy_meters":p.point.horizontal_accuracy_meters}}));
    }
    let doc = serde_json::json!({"type":"FeatureCollection","features":features});
    serde_json::to_writer(
        crate::LimitedWriter::new(io::sink(), limits.max_bytes),
        &doc,
    )
    .map_err(io::Error::other)?;
    Ok(doc)
}
pub(crate) fn escape_xml(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '"' => "&quot;".into(),
            '\'' => "&apos;".into(),
            c if c.is_control() && !matches!(c, '\n' | '\r' | '\t') => "\u{fffd}".into(),
            _ => c.to_string(),
        })
        .collect()
}
pub fn kml(samples: &[GeoSample], limits: AnalysisLimits) -> io::Result<String> {
    if samples.len() > limits.max_results {
        return Err(invalid("KML point budget exceeded"));
    }
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><kml xmlns=\"http://www.opengis.net/kml/2.2\"><Document>",
    );
    for p in samples {
        if !p.point.is_valid() {
            return Err(invalid("invalid KML coordinate"));
        }
        let coordinates = match p.point.altitude_meters {
            Some(a) => format!("{},{},{}", p.point.longitude, p.point.latitude, a),
            None => format!("{},{}", p.point.longitude, p.point.latitude),
        };
        out.push_str(&format!("<Placemark><name>{}</name><Point><coordinates>{coordinates}</coordinates></Point></Placemark>",escape_xml(&p.path.to_string())));
        if out.len() > limits.max_bytes {
            return Err(invalid("KML byte budget exceeded"));
        }
    }
    out.push_str("</Document></kml>");
    if out.len() > limits.max_bytes {
        return Err(invalid("KML byte budget exceeded"));
    }
    Ok(out)
}
