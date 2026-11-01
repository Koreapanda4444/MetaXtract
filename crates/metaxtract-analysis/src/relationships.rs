use crate::{AnalysisLimits, check_record, invalid, schema::*};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum RelationshipBasis {
    ContentDigest {
        algorithm: DigestAlgorithm,
        digest: String,
    },
    SharedMetadata {
        key: CanonicalKey,
        value: NormalizedValue,
    },
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Relationship {
    pub first: usize,
    pub second: usize,
    pub basis: RelationshipBasis,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RelationshipGraph {
    pub files: Vec<EvidencePath>,
    pub edges: Vec<Relationship>,
}
/// Groups use a deterministic spanning star, avoiding quadratic expansion of common values.
pub fn build(
    records: &[ExtractionRecord],
    limits: AnalysisLimits,
) -> io::Result<RelationshipGraph> {
    if records.len() > limits.max_records {
        return Err(invalid("relationship record budget exceeded"));
    }
    let mut sorted: Vec<_> = records.iter().collect();
    sorted.sort_by_key(|r| r.source.path.to_string());
    let mut files = vec![];
    let mut paths = BTreeSet::new();
    let mut groups: BTreeMap<String, (RelationshipBasis, BTreeSet<usize>)> = BTreeMap::new();
    let mut bytes = 0usize;
    for (i, r) in sorted.iter().enumerate() {
        check_record(r, limits)?;
        if !paths.insert(r.source.path.to_string()) {
            return Err(invalid(
                "relationship graph requires unique paths in one scan",
            ));
        }
        files.push(r.source.path.clone());
        for d in &r.digests {
            if matches!(
                d.algorithm,
                DigestAlgorithm::Sha256 | DigestAlgorithm::Blake3
            ) {
                if d.bytes.len() != 32 {
                    return Err(invalid("invalid relationship digest length"));
                }
                let digest = crate::hex(&d.bytes);
                let k = format!(
                    "digest:{:?}:{digest}:{}",
                    d.algorithm, r.source.identity.size_bytes
                );
                groups
                    .entry(k)
                    .or_insert_with(|| {
                        (
                            RelationshipBasis::ContentDigest {
                                algorithm: d.algorithm.clone(),
                                digest,
                            },
                            BTreeSet::new(),
                        )
                    })
                    .1
                    .insert(i);
            }
        }
        for c in &r.claims {
            if c.resolution == ClaimResolution::Rejected || matches!(c.value, NormalizedValue::Null)
            {
                continue;
            }
            let value = serde_json::to_string(&c.value).map_err(io::Error::other)?;
            bytes = bytes.saturating_add(value.len() + c.key.as_str().len() + 64);
            if bytes > limits.max_bytes {
                return Err(invalid("relationship metadata byte budget exceeded"));
            }
            let k = format!("claim:{}:{value}", c.key);
            groups
                .entry(k)
                .or_insert_with(|| {
                    (
                        RelationshipBasis::SharedMetadata {
                            key: c.key.clone(),
                            value: c.value.clone(),
                        },
                        BTreeSet::new(),
                    )
                })
                .1
                .insert(i);
        }
        if groups.len() > limits.max_fields {
            return Err(invalid("relationship group budget exceeded"));
        }
    }
    let mut edges = vec![];
    for (_, (basis, members)) in groups {
        if let Some(first) = members.first() {
            for second in members.iter().skip(1) {
                if edges.len() >= limits.max_results {
                    return Err(invalid(
                        "relationship edge budget exceeded; narrow the file selection",
                    ));
                }
                bytes = bytes.saturating_add(crate::encoded_size(&basis, limits.max_bytes)?);
                if bytes > limits.max_bytes {
                    return Err(invalid("relationship edge byte budget exceeded"));
                }
                edges.push(Relationship {
                    first: *first,
                    second: *second,
                    basis: basis.clone(),
                });
            }
        }
    }
    Ok(RelationshipGraph { files, edges })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn record(path: &str) -> ExtractionRecord {
        ExtractionRecord::new(SourceDescriptor {
            path: EvidencePath::root(RelativePath::parse(path).unwrap()),
            kind: SourceKind::File,
            identity: FileIdentity {
                size_bytes: 0,
                modified_ns: None,
                created_ns: None,
                stable_id: None,
            },
            declared_media_type: None,
            detected_media_type: None,
        })
    }
    #[test]
    fn digest_links_are_deterministic_and_budgeted() {
        let mut a = record("a");
        let mut b = record("b");
        let digest = ContentDigest {
            algorithm: DigestAlgorithm::Sha256,
            bytes: vec![7; 32],
        };
        a.digests.push(digest.clone());
        b.digests.push(digest);
        let g = build(&[b.clone(), a.clone()], Default::default()).unwrap();
        assert_eq!(g.files[0], a.source.path);
        assert_eq!(g.edges.len(), 1);
        assert!(matches!(
            g.edges[0].basis,
            RelationshipBasis::ContentDigest { .. }
        ));
        assert!(
            build(
                &[a, b],
                AnalysisLimits {
                    max_results: 0,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
}
