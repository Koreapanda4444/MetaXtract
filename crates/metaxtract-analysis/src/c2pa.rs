use crate::{AnalysisLimits, check_record, encoded_size, invalid, schema::*};
use ciborium::Value;
use std::{collections::BTreeMap, io};

/// Ordered map pairs preserve duplicate/non-text keys. Numeric encodings may normalize;
/// the complete original CBOR encoding remains in the linked payload observation.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum C2paValue {
    Integer(String),
    Bytes(String),
    Text(String),
    FloatBits(u64),
    Bool(bool),
    Null,
    Array(Vec<C2paValue>),
    Map(Vec<(C2paValue, C2paValue)>),
    Tag { number: u64, value: Box<C2paValue> },
    JsonText(String),
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct C2paBox {
    pub identity: String,
    pub parent: Option<String>,
    pub label: Option<String>,
    pub box_type: String,
    pub uuid: Option<String>,
    pub offset: u64,
    pub size: u64,
    pub payload_bytes: usize,
    pub payload_observation: Option<ObservationId>,
    pub decoded: Option<C2paValue>,
    pub issue: Option<String>,
    pub supporting_observations: Vec<ObservationId>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct C2paEvidence {
    pub source: EvidencePath,
    pub boxes: Vec<C2paBox>,
    pub issues: Vec<String>,
    pub signature_integrity: String,
    pub content_binding: String,
    pub certificate_trust: String,
}
fn value(o: &MetadataObservation) -> &RawValue {
    o.interpreted_value.as_ref().unwrap_or(&o.raw_value)
}
fn text(v: &RawValue) -> Option<&str> {
    if let RawValue::Text(v) = v {
        v.lossless.then_some(v.text.as_str())
    } else {
        None
    }
}
fn number(v: &RawValue) -> Option<u64> {
    match v {
        RawValue::Unsigned(v) => Some(*v),
        RawValue::Signed(v) => (*v).try_into().ok(),
        _ => None,
    }
}
fn projected(value: Value, remaining: &mut usize, depth: usize) -> io::Result<C2paValue> {
    if *remaining == 0 || depth > 32 {
        return Err(invalid("C2PA semantic field or depth budget exceeded"));
    }
    *remaining -= 1;
    Ok(match value {
        Value::Integer(v) => C2paValue::Integer(i128::from(v).to_string()),
        Value::Bytes(v) => C2paValue::Bytes(crate::hex(&v)),
        Value::Text(v) => C2paValue::Text(v),
        Value::Float(v) => C2paValue::FloatBits(v.to_bits()),
        Value::Bool(v) => C2paValue::Bool(v),
        Value::Null => C2paValue::Null,
        Value::Array(v) => C2paValue::Array(
            v.into_iter()
                .map(|v| projected(v, remaining, depth + 1))
                .collect::<io::Result<_>>()?,
        ),
        Value::Map(v) => C2paValue::Map(
            v.into_iter()
                .map(|(k, v)| {
                    Ok((
                        projected(k, remaining, depth + 1)?,
                        projected(v, remaining, depth + 1)?,
                    ))
                })
                .collect::<io::Result<_>>()?,
        ),
        Value::Tag(number, v) => C2paValue::Tag {
            number,
            value: Box::new(projected(*v, remaining, depth + 1)?),
        },
        _ => return Err(invalid("unsupported CBOR value")),
    })
}
fn decode(bytes: &[u8], kind: &str, remaining: &mut usize) -> io::Result<C2paValue> {
    if bytes.len() > 512 * 1024 {
        return Err(invalid(
            "C2PA semantic packet exceeds 512 KiB; original bytes retained",
        ));
    }
    if kind == "json" {
        let _: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|_| invalid("malformed C2PA JSON assertion; raw packet retained"))?;
        return Ok(C2paValue::JsonText(
            std::str::from_utf8(bytes)
                .map_err(|_| invalid("C2PA JSON is not UTF-8"))?
                .into(),
        ));
    }
    let mut cursor = io::Cursor::new(bytes);
    let value: Value = ciborium::de::from_reader_with_recursion_limit(&mut cursor, 32)
        .map_err(|_| invalid("malformed or over-deep C2PA CBOR; raw packet retained"))?;
    if cursor.position() != bytes.len() as u64 {
        return Err(invalid("trailing C2PA CBOR bytes; raw packet retained"));
    }
    projected(value, remaining, 0)
}
pub fn extract(
    records: &[ExtractionRecord],
    limits: AnalysisLimits,
) -> io::Result<Vec<C2paEvidence>> {
    if records.len() > limits.max_records {
        return Err(invalid("C2PA record budget exceeded"));
    }
    let mut out = Vec::new();
    let mut bytes = 0usize;
    let mut remaining = limits.max_fields.min(20000);
    for record in records {
        check_record(record, limits)?;
        bytes = bytes.saturating_add(encoded_size(record, limits.max_bytes)?);
        if bytes > limits.max_bytes {
            return Err(invalid("C2PA input byte budget exceeded"));
        }
        let mut groups: BTreeMap<&str, Vec<&MetadataObservation>> = BTreeMap::new();
        let mut issues = Vec::new();
        for observation in &record.observations {
            if observation.provenance.parser.id != "c2pa.manifest" {
                continue;
            }
            if observation.tag.namespace.starts_with("diagnostic.") {
                if let Some(v) = text(value(observation)) {
                    issues.push(v.into());
                }
            }
            if observation.tag.namespace.starts_with("c2pa.store")
                && observation.tag.namespace.contains(".box")
            {
                groups
                    .entry(&observation.tag.namespace)
                    .or_default()
                    .push(observation);
            }
        }
        if groups.is_empty() && issues.is_empty() {
            continue;
        }
        let labels: BTreeMap<_, _> = groups
            .iter()
            .map(|(id, observations)| {
                (
                    *id,
                    observations
                        .iter()
                        .find(|o| o.tag.name.as_deref() == Some("box_label"))
                        .and_then(|o| text(value(o))),
                )
            })
            .collect();
        let mut boxes = Vec::new();
        for (identity, observations) in &groups {
            if boxes.len() >= limits.max_results {
                return Err(invalid("C2PA box result budget exceeded"));
            }
            let field = |key: &str| {
                observations
                    .iter()
                    .find(|o| o.tag.name.as_deref() == Some(key))
                    .copied()
            };
            let read = |key: &str| field(key).and_then(|o| text(value(o)));
            let parent = field("parent_index")
                .and_then(|o| number(value(o)))
                .filter(|n| *n != u64::MAX)
                .map(|id| format!("{}.box{id}", identity.split(".box").next().unwrap_or("")));
            let kind = read("box_type").unwrap_or("unknown");
            let payload = field("box_payload");
            let data = payload.and_then(|o| {
                if let RawValue::Bytes(bytes) = value(o) {
                    Some(bytes.as_slice())
                } else {
                    None
                }
            });
            let (decoded, issue) = if matches!(kind, "cbor" | "json")
                && parent
                    .as_deref()
                    .and_then(|p| labels.get(p))
                    .copied()
                    .flatten()
                    != Some("c2pa.signature")
            {
                match data.map(|data| decode(data, kind, &mut remaining)) {
                    Some(Ok(value)) => (Some(value), None),
                    Some(Err(error)) => (None, Some(error.to_string())),
                    None => (None, Some("raw payload unavailable".into())),
                }
            } else {
                (None, None)
            };
            boxes.push(C2paBox {
                identity: (*identity).into(),
                parent,
                label: read("box_label").map(str::to_owned),
                box_type: kind.into(),
                uuid: read("box_uuid").map(str::to_owned),
                offset: field("box_offset")
                    .and_then(|o| number(value(o)))
                    .unwrap_or(0),
                size: field("box_size")
                    .and_then(|o| number(value(o)))
                    .unwrap_or(0),
                payload_bytes: data.map_or(0, |b| b.len()),
                payload_observation: payload.map(|o| o.id),
                decoded,
                issue,
                supporting_observations: observations.iter().map(|o| o.id).collect(),
            });
        }
        if out.len() >= limits.max_results {
            return Err(invalid("C2PA result budget exceeded"));
        }
        out.push(C2paEvidence {
            source: record.source.path.clone(),
            boxes,
            issues,
            signature_integrity: "unverified".into(),
            content_binding: "unverified".into(),
            certificate_trust: "unknown".into(),
        });
    }
    out.sort_by_key(|r| r.source.to_string());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cbor_preserves_duplicate_keys_tags_and_exact_values_and_rejects_trailing_or_deep_data() {
        let value = decode(b"\xa2\x61x\x01\x61x\xd8\x2a\x41\xff", "cbor", &mut 100).unwrap();
        let C2paValue::Map(pairs) = value else {
            panic!()
        };
        assert_eq!(pairs.len(), 2);
        assert!(matches!(pairs[1].1, C2paValue::Tag { number: 42, .. }));
        assert!(
            matches!(decode(b"\xc2\x41\xff", "cbor", &mut 100).unwrap(), C2paValue::Integer(v) if v == "255")
        );
        assert!(decode(b"\x01\x02", "cbor", &mut 100).is_err());
        let mut deep = vec![0x81; 40];
        deep.push(0);
        assert!(decode(&deep, "cbor", &mut 100).is_err());
        assert!(decode(b"\x82\x01\x02", "cbor", &mut 1).is_err());
        assert!(decode(&vec![0; 512 * 1024 + 1], "cbor", &mut 100).is_err());
    }
}
