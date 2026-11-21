use crate::{AnalysisLimits, check_record, invalid, schema::*};
use std::{collections::BTreeMap, io};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CertificatePathStatus {
    ValidToConfiguredAnchor,
    Untrusted,
    Unknown,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PdfSignatureAssessment {
    pub source: EvidencePath,
    pub signature: String,
    pub integrity: String,
    pub certificate_path: CertificatePathStatus,
    pub certificate_detail: String,
    pub covers_current_file: bool,
    pub unsigned_tail_bytes: Option<u64>,
    pub evaluation_unix_seconds: Option<i64>,
    pub revocation: String,
    pub timestamp: String,
    pub certificates: BTreeMap<String, String>,
    pub supporting_observations: Vec<ObservationId>,
}
fn value(observation: &MetadataObservation) -> &RawValue {
    observation
        .interpreted_value
        .as_ref()
        .unwrap_or(&observation.raw_value)
}
fn text(value: &RawValue) -> Option<&str> {
    if let RawValue::Text(value) = value {
        value.lossless.then_some(value.text.as_str())
    } else {
        None
    }
}
fn number(value: &RawValue) -> Option<u64> {
    match value {
        RawValue::Signed(n) => (*n).try_into().ok(),
        RawValue::Unsigned(n) => Some(*n),
        _ => None,
    }
}

/// Preserves the evaluator's scope. A valid path never upgrades an unverified CMS signature.
pub fn assess(
    records: &[ExtractionRecord],
    limits: AnalysisLimits,
) -> io::Result<Vec<PdfSignatureAssessment>> {
    if records.len() > limits.max_records {
        return Err(invalid("PDF signature record budget exceeded"));
    }
    let mut reports = Vec::new();
    let mut bytes = 0usize;
    for record in records {
        check_record(record, limits)?;
        bytes = bytes.saturating_add(crate::encoded_size(record, limits.max_bytes)?);
        if bytes > limits.max_bytes {
            return Err(invalid("PDF signature byte budget exceeded"));
        }
        let mut signatures: BTreeMap<&str, Vec<&MetadataObservation>> = BTreeMap::new();
        for observation in &record.observations {
            if observation.provenance.parser.id == "pdf.security"
                && observation.tag.namespace.starts_with("pdf.signature")
            {
                signatures
                    .entry(&observation.tag.namespace)
                    .or_default()
                    .push(observation);
            }
        }
        for (signature, observations) in signatures {
            if reports.len() >= limits.max_results {
                return Err(invalid("PDF signature result budget exceeded"));
            }
            let field = |key: &str| {
                let mut candidates = observations
                    .iter()
                    .filter(|o| o.tag.name.as_deref() == Some(key));
                let first = value(candidates.next()?);
                candidates.all(|o| value(o) == first).then_some(first)
            };
            let read = |key: &str| field(key).and_then(text);
            let integrity = read("cms_integrity").unwrap_or("unverified");
            let detail = read("certificate_trust").unwrap_or("unknown_not_evaluated");
            let certificate_path = if integrity != "valid_for_signed_revision" {
                CertificatePathStatus::Unknown
            } else {
                match detail {
                    "chain_valid_to_configured_anchor" => {
                        CertificatePathStatus::ValidToConfiguredAnchor
                    }
                    "untrusted_or_invalid_chain" | "invalid_signing_key_usage" => {
                        CertificatePathStatus::Untrusted
                    }
                    _ => CertificatePathStatus::Unknown,
                }
            };
            let unsigned_tail_bytes = field("unsigned_tail_bytes").and_then(number);
            let certificates = observations
                .iter()
                .filter_map(|o| {
                    let key = o.tag.name.as_deref()?;
                    if !key.starts_with("signer") && !key.starts_with("trust_anchor") {
                        return None;
                    }
                    Some((key.to_owned(), text(value(o))?.to_owned()))
                })
                .collect();
            reports.push(PdfSignatureAssessment {
                source: record.source.path.clone(),
                signature: signature.into(),
                integrity: integrity.into(),
                certificate_path,
                certificate_detail: detail.into(),
                covers_current_file: integrity == "valid_for_signed_revision"
                    && unsigned_tail_bytes == Some(0)
                    && field("covered_end_is_current_eof").and_then(number) == Some(1),
                unsigned_tail_bytes,
                evaluation_unix_seconds: read("certificate_evaluation_unix_seconds")
                    .and_then(|n| n.parse().ok()),
                revocation: read("revocation_status")
                    .unwrap_or("unknown_not_checked")
                    .into(),
                timestamp: read("timestamp_status")
                    .unwrap_or("unknown_not_checked")
                    .into(),
                certificates,
                supporting_observations: observations.iter().map(|o| o.id).collect(),
            });
        }
    }
    reports.sort_by_cached_key(|r| (r.source.to_string(), r.signature.clone()));
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text_value(value: &str) -> TextValue {
        TextValue {
            text: value.into(),
            original_encoding: None,
            language: None,
            encoded_bytes: None,
            lossless: true,
        }
    }
    fn record(integrity: &str, trust: &str, tail: u64) -> ExtractionRecord {
        let source = EvidencePath::root(RelativePath::parse("signed.pdf").unwrap());
        let mut record = ExtractionRecord::new(SourceDescriptor {
            path: source.clone(),
            kind: SourceKind::File,
            identity: FileIdentity {
                size_bytes: 100,
                modified_ns: None,
                created_ns: None,
                stable_id: None,
            },
            declared_media_type: None,
            detected_media_type: None,
        });
        for (key, value) in [
            ("cms_integrity", RawValue::Text(text_value(integrity))),
            ("certificate_trust", RawValue::Text(text_value(trust))),
            ("unsigned_tail_bytes", RawValue::Unsigned(tail)),
            (
                "covered_end_is_current_eof",
                RawValue::Unsigned(u64::from(tail == 0)),
            ),
        ] {
            let id = ObservationId::new(record.observations.len() as u64);
            record.observations.push(MetadataObservation {
                id,
                tag: TagIdentity {
                    namespace: "pdf.signature0".into(),
                    code: TagCode::Text(key.into()),
                    name: Some(key.into()),
                },
                raw_value: value,
                interpreted_value: None,
                unit: None,
                provenance: Provenance {
                    parser: ParserDescriptor {
                        id: "pdf.security".into(),
                        name: "test".into(),
                        version: "test".into(),
                        library: None,
                    },
                    source: source.clone(),
                    stream_id: None,
                    source_range: None,
                    method: ExtractionMethod::NativeLibrary,
                    sequence: id.get(),
                },
            });
        }
        record
    }
    #[test]
    fn trust_never_upgrades_failed_integrity_or_unsigned_tail_and_sources_are_preserved() {
        let records = [
            record("invalid", "chain_valid_to_configured_anchor", 0),
            record("valid_for_signed_revision", "unknown_no_trust_anchors", 12),
        ];
        let before = serde_json::to_vec(&records).unwrap();
        let reports = assess(&records, Default::default()).unwrap();
        assert_eq!(reports[0].certificate_path, CertificatePathStatus::Unknown);
        assert!(!reports[0].covers_current_file);
        assert_eq!(reports[1].certificate_path, CertificatePathStatus::Unknown);
        assert!(!reports[1].covers_current_file);
        assert_eq!(reports[1].unsigned_tail_bytes, Some(12));
        assert!(
            reports.iter().all(
                |r| r.revocation.starts_with("unknown") && r.supporting_observations.len() == 4
            )
        );
        assert_eq!(before, serde_json::to_vec(&records).unwrap());
        assert!(
            assess(
                &records,
                AnalysisLimits {
                    max_results: 1,
                    ..Default::default()
                }
            )
            .is_err()
        );
        let valid = assess(
            &[record(
                "valid_for_signed_revision",
                "chain_valid_to_configured_anchor",
                0,
            )],
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            valid[0].certificate_path,
            CertificatePathStatus::ValidToConfiguredAnchor
        );
        assert!(valid[0].covers_current_file);
        let mut contradictory = record(
            "valid_for_signed_revision",
            "chain_valid_to_configured_anchor",
            0,
        );
        let mut alternative = contradictory.observations[1].clone();
        alternative.id = ObservationId::new(4);
        alternative.provenance.sequence = 4;
        alternative.raw_value = RawValue::Text(text_value("untrusted_or_invalid_chain"));
        contradictory.observations.push(alternative);
        assert_eq!(
            assess(&[contradictory], Default::default()).unwrap()[0].certificate_path,
            CertificatePathStatus::Unknown
        );
    }
}
