use crate::{AnalysisLimits, encoded_size, invalid, relationships::*, schema::*};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum AssociationBasis {
    XmpFilenameCandidate {
        ambiguous: bool,
    },
    XmpDeclaredDocumentIdentifier {
        identifier: String,
        source_observation: ObservationId,
        sidecar_observation: ObservationId,
    },
}

fn sidecar(record: &ExtractionRecord) -> bool {
    record
        .observations
        .iter()
        .any(|o| o.provenance.parser.id == "xmp.sidecar" && o.tag.name.as_deref() == Some("packet"))
}
fn identifiers(record: &ExtractionRecord) -> BTreeMap<String, ObservationId> {
    record
        .observations
        .iter()
        .filter_map(|o| {
            let name = o.tag.name.as_deref()?;
            if !matches!(
                name,
                "{http://ns.adobe.com/xap/1.0/mm/}DocumentID"
                    | "{http://ns.adobe.com/xap/1.0/mm/}OriginalDocumentID"
            ) {
                return None;
            }
            match o.interpreted_value.as_ref().unwrap_or(&o.raw_value) {
                RawValue::Text(t) if t.lossless && !t.text.trim().is_empty() => {
                    Some((t.text.trim().to_owned(), o.id))
                }
                _ => None,
            }
        })
        .collect()
}
fn charge(bytes: &mut usize, amount: usize, limits: AnalysisLimits) -> io::Result<()> {
    *bytes = bytes.saturating_add(amount);
    if *bytes > limits.max_bytes {
        return Err(invalid("association byte budget exceeded"));
    }
    Ok(())
}
pub(crate) fn append_xmp(
    records: &[&ExtractionRecord],
    graph: &mut RelationshipGraph,
    limits: AnalysisLimits,
    bytes: &mut usize,
) -> io::Result<()> {
    let mut names: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    let mut ids: BTreeMap<String, Vec<(usize, ObservationId)>> = BTreeMap::new();
    let mut metadata = vec![];
    for (index, record) in records.iter().enumerate() {
        // Cross-container filename matches are deliberately excluded.
        if record.source.path.depth() != 0 || record.source.kind != SourceKind::File {
            continue;
        }
        if sidecar(record) {
            metadata.push(index);
            continue;
        }
        let path = record.source.path.root_path().as_str();
        charge(
            bytes,
            path.len().saturating_mul(2).saturating_add(256),
            limits,
        )?;
        names.entry(path.to_owned()).or_default().insert(index);
        let (parent, leaf) = path.rsplit_once('/').unwrap_or(("", path));
        if let Some((stem, _)) = leaf.rsplit_once('.') {
            names
                .entry(if parent.is_empty() {
                    stem.to_owned()
                } else {
                    format!("{parent}/{stem}")
                })
                .or_default()
                .insert(index);
        }
        for (id, observation) in identifiers(record) {
            charge(bytes, id.len().saturating_add(128), limits)?;
            ids.entry(id).or_default().push((index, observation));
        }
    }
    let mut comparisons = 0usize;
    let mut retain =
        |source: usize, metadata: usize, evidence: AssociationBasis| -> io::Result<()> {
            comparisons = comparisons.saturating_add(1);
            if comparisons > limits.max_comparisons || graph.edges.len() >= limits.max_results {
                return Err(invalid(
                    "association result budget exceeded; narrow the selection",
                ));
            }
            let basis = RelationshipBasis::ExternalMetadata {
                metadata_file: metadata,
                evidence,
            };
            charge(bytes, encoded_size(&basis, limits.max_bytes)?, limits)?;
            graph.edges.push(Relationship {
                first: source,
                second: metadata,
                basis,
            });
            Ok(())
        };
    for index in metadata {
        let record = records[index];
        let path = record.source.path.root_path().as_str();
        if path.to_ascii_lowercase().ends_with(".xmp") {
            let stem = &path[..path.len() - 4];
            if let Some(targets) = names.get(stem) {
                for target in targets {
                    retain(
                        *target,
                        index,
                        AssociationBasis::XmpFilenameCandidate {
                            ambiguous: targets.len() > 1,
                        },
                    )?;
                }
            }
        }
        for (identifier, sidecar_observation) in identifiers(record) {
            if let Some(targets) = ids.get(&identifier) {
                for (target, source_observation) in targets {
                    retain(
                        *target,
                        index,
                        AssociationBasis::XmpDeclaredDocumentIdentifier {
                            identifier: identifier.clone(),
                            source_observation: *source_observation,
                            sidecar_observation,
                        },
                    )?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(path: &str, xmp: bool) -> ExtractionRecord {
        let mut record = ExtractionRecord::new(SourceDescriptor {
            path: EvidencePath::root(RelativePath::parse(path).unwrap()),
            kind: SourceKind::File,
            identity: FileIdentity {
                size_bytes: 10,
                modified_ns: None,
                created_ns: None,
                stable_id: None,
            },
            declared_media_type: None,
            detected_media_type: None,
        });
        if xmp {
            field(&mut record, "packet", "raw packet", "xmp.sidecar");
        }
        record
    }
    fn field(record: &mut ExtractionRecord, name: &str, value: &str, parser: &str) {
        let id = ObservationId::new(record.observations.len() as u64);
        record.observations.push(MetadataObservation {
            id,
            tag: TagIdentity {
                namespace: "test.xmp".into(),
                code: TagCode::Text(name.into()),
                name: Some(name.into()),
            },
            raw_value: RawValue::Text(TextValue {
                text: value.into(),
                original_encoding: None,
                language: None,
                encoded_bytes: None,
                lossless: true,
            }),
            interpreted_value: None,
            unit: None,
            provenance: Provenance {
                parser: ParserDescriptor {
                    id: parser.into(),
                    name: parser.into(),
                    version: "test".into(),
                    library: None,
                },
                source: record.source.path.clone(),
                stream_id: None,
                source_range: None,
                method: ExtractionMethod::DecodedPayload,
                sequence: id.get(),
            },
        });
    }
    #[test]
    fn filename_candidates_preserve_ambiguity_scope_and_original_records() {
        let records = vec![
            record("photos/a.jpg", false),
            record("photos/a.raw", false),
            record("photos/a.xmp", true),
            record("other/a.jpg", false),
        ];
        let before = serde_json::to_vec(&records).unwrap();
        let graph = build(&records, Default::default()).unwrap();
        assert_eq!(graph.edges.len(), 2);
        for edge in graph.edges {
            assert!(matches!(
                edge.basis,
                RelationshipBasis::ExternalMetadata {
                    evidence: AssociationBasis::XmpFilenameCandidate { ambiguous: true },
                    ..
                }
            ));
        }
        assert_eq!(before, serde_json::to_vec(&records).unwrap());
    }
    #[test]
    fn document_identifiers_retain_both_observation_ids_and_budget() {
        let mut image = record("a.jpg", false);
        let mut xmp = record("sidecars/custom.xmp", true);
        let key = "{http://ns.adobe.com/xap/1.0/mm/}DocumentID";
        field(&mut image, key, "uuid:test", "images.metadata");
        field(&mut xmp, key, "uuid:test", "xmp.sidecar");
        let records = [image, xmp];
        let graph = build(&records, Default::default()).unwrap();
        assert_eq!(graph.edges.len(), 1);
        assert!(matches!(
            graph.edges[0].basis,
            RelationshipBasis::ExternalMetadata {
                evidence: AssociationBasis::XmpDeclaredDocumentIdentifier { .. },
                ..
            }
        ));
        assert!(
            build(
                &records,
                AnalysisLimits {
                    max_results: 0,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
}
