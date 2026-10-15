use crate::{Result, invalid};
use metaxtract_schema::*;
use serde_json::Value;

pub(super) fn convert(value: &Value) -> Result<ExtractionRecord> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("legacy record must be an object"))?;
    let path = object
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("legacy record path is missing"))?;
    let path =
        EvidencePath::root(RelativePath::parse(path.to_owned()).map_err(std::io::Error::other)?);
    let size = object
        .get("size_bytes")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid("invalid legacy evidence size"))?;
    let mime = object
        .get("mime")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| invalid("invalid legacy MIME"))?;
    let mut record = ExtractionRecord::new(SourceDescriptor {
        path: path.clone(),
        kind: SourceKind::File,
        identity: FileIdentity {
            size_bytes: size,
            modified_ns: None,
            created_ns: None,
            stable_id: None,
        },
        declared_media_type: None,
        detected_media_type: Some(mime.into()),
    });
    let metadata = object
        .get("metadata")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("legacy metadata must be an object"))?;
    for (index, (key, value)) in metadata.iter().enumerate() {
        record.observations.push(MetadataObservation {
            id: ObservationId::new(index as u64 + 1),
            tag: TagIdentity {
                namespace: "legacy.python".into(),
                code: TagCode::Text(key.clone()),
                name: Some(key.clone()),
            },
            raw_value: raw(value),
            interpreted_value: None,
            unit: None,
            provenance: Provenance {
                parser: ParserDescriptor {
                    id: "legacy-python-import".into(),
                    name: "Python reference import".into(),
                    version: "1".into(),
                    library: None,
                },
                source: path.clone(),
                stream_id: None,
                source_range: None,
                method: ExtractionMethod::Direct,
                sequence: index as u64,
            },
        });
    }
    let mut has_errors = false;
    for (field, severity) in [
        ("warnings", DiagnosticSeverity::Warning),
        ("errors", DiagnosticSeverity::Error),
    ] {
        let messages = object
            .get(field)
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("legacy diagnostics must be arrays"))?;
        for message in messages {
            let message = message
                .as_str()
                .ok_or_else(|| invalid("legacy diagnostic must be text"))?;
            has_errors |= severity == DiagnosticSeverity::Error;
            record.diagnostics.push(Diagnostic {
                code: format!("legacy.{field}"),
                severity,
                stage: DiagnosticStage::Parse,
                message: message.into(),
                parser: None,
                source: path.clone(),
                source_range: None,
                related_observations: vec![],
                recoverable: true,
            });
        }
    }
    let digest = object
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("legacy SHA-256 is missing"))?;
    if digest.is_empty() && has_errors {
        record.status = ExtractionStatus::Failed;
    } else {
        if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid("invalid legacy SHA-256"));
        }
        let bytes = (0..32)
            .map(|i| {
                u8::from_str_radix(&digest[i * 2..i * 2 + 2], 16).map_err(std::io::Error::other)
            })
            .collect::<Result<Vec<_>>>()?;
        record.digests.push(ContentDigest {
            algorithm: DigestAlgorithm::Sha256,
            bytes,
        });
        if has_errors {
            record.status = ExtractionStatus::Partial;
        }
    }
    Ok(record)
}
fn raw(value: &Value) -> RawValue {
    match value {
        Value::Null => RawValue::Null,
        Value::Bool(v) => RawValue::Bool(*v),
        Value::String(v) => RawValue::Text(TextValue {
            text: v.clone(),
            original_encoding: Some("UTF-8".into()),
            language: None,
            encoded_bytes: None,
            lossless: true,
        }),
        Value::Number(v) => {
            if let Some(n) = v.as_i64() {
                RawValue::Signed(n)
            } else if let Some(n) = v.as_u64() {
                RawValue::Unsigned(n)
            } else {
                RawValue::Text(TextValue {
                    text: v.to_string(),
                    original_encoding: Some("JSON number".into()),
                    language: None,
                    encoded_bytes: None,
                    lossless: true,
                })
            }
        }
        Value::Array(v) => RawValue::Sequence(v.iter().map(raw).collect()),
        Value::Object(v) => RawValue::Structure(
            v.iter()
                .map(|(k, v)| RawField {
                    name: k.clone(),
                    value: raw(v),
                })
                .collect(),
        ),
    }
}
