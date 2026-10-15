use crate::{
    AnalysisLimits, check_record, hex, invalid,
    schema::*,
    validation::{ValueRelation, relation},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum FieldSegment {
    Key(String),
    Index(usize),
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FieldChange {
    pub field: Vec<FieldSegment>,
    pub before: Option<Value>,
    pub after: Option<Value>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ClaimChange {
    pub key: CanonicalKey,
    pub before: Vec<NormalizedClaim>,
    pub after: Vec<NormalizedClaim>,
    pub semantically_equal: bool,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RecordDiff {
    pub before_path: EvidencePath,
    pub after_path: EvidencePath,
    pub content_changed: Option<bool>,
    pub fields: Vec<FieldChange>,
    pub claims: Vec<ClaimChange>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Relocation {
    pub before: EvidencePath,
    pub after: EvidencePath,
    pub sha256: String,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ScanDiff {
    pub added: Vec<EvidencePath>,
    pub removed: Vec<EvidencePath>,
    pub changed: Vec<RecordDiff>,
    pub relocations: Vec<Relocation>,
}
fn blob(bytes: &[u8]) -> Value {
    json!({"length":bytes.len(),"sha256":hex(&Sha256::digest(bytes))})
}
fn compact_raw(value: &RawValue) -> Value {
    match value {
        RawValue::Bytes(v) => json!({"Bytes":blob(v)}),
        RawValue::Text(t) => {
            json!({"Text":{"text":t.text,"encoding":t.original_encoding,"language":t.language,"lossless":t.lossless,"encoded_bytes":t.encoded_bytes.as_deref().map(blob)}})
        }
        RawValue::Sequence(v) => json!({"Sequence":v.iter().map(compact_raw).collect::<Vec<_>>()}),
        RawValue::Structure(v) => {
            json!({"Structure":v.iter().map(|f|json!({"name":f.name,"value":compact_raw(&f.value)})).collect::<Vec<_>>()})
        }
        _ => serde_json::to_value(value).expect("raw scalar serialization is infallible"),
    }
}
fn view(record: &ExtractionRecord) -> Value {
    json!({"source":record.source,"digests":record.digests,"containers":record.containers,"status":record.status,"diagnostics":record.diagnostics,"conflicts":record.conflicts,"observations":record.observations.iter().map(|o|json!({"id":o.id,"tag":o.tag,"raw_value":compact_raw(&o.raw_value),"interpreted_value":o.interpreted_value.as_ref().map(compact_raw),"unit":o.unit,"provenance":o.provenance})).collect::<Vec<_>>()})
}
fn walk(
    a: Option<&Value>,
    b: Option<&Value>,
    path: &mut Vec<FieldSegment>,
    out: &mut Vec<FieldChange>,
    limits: AnalysisLimits,
) -> io::Result<()> {
    if a == b {
        return Ok(());
    }
    match (a, b) {
        (Some(Value::Object(a)), Some(Value::Object(b))) => {
            let keys: BTreeSet<_> = a.keys().chain(b.keys()).collect();
            for key in keys {
                path.push(FieldSegment::Key(key.clone()));
                walk(a.get(key), b.get(key), path, out, limits)?;
                path.pop();
            }
        }
        (Some(Value::Array(a)), Some(Value::Array(b))) => {
            for i in 0..a.len().max(b.len()) {
                path.push(FieldSegment::Index(i));
                walk(a.get(i), b.get(i), path, out, limits)?;
                path.pop();
            }
        }
        _ => out.push(FieldChange {
            field: path.clone(),
            before: a.cloned(),
            after: b.cloned(),
        }),
    }
    if out.len() > limits.max_results {
        return Err(invalid("field diff result budget exceeded"));
    }
    Ok(())
}
pub fn compare_records(
    before: &ExtractionRecord,
    after: &ExtractionRecord,
    limits: AnalysisLimits,
) -> io::Result<RecordDiff> {
    check_record(before, limits)?;
    check_record(after, limits)?;
    let a = view(before);
    let b = view(after);
    let mut fields = vec![];
    walk(Some(&a), Some(&b), &mut vec![], &mut fields, limits)?;
    let mut grouped: BTreeMap<CanonicalKey, (Vec<NormalizedClaim>, Vec<NormalizedClaim>)> =
        BTreeMap::new();
    for c in &before.claims {
        grouped.entry(c.key.clone()).or_default().0.push(c.clone());
    }
    for c in &after.claims {
        grouped.entry(c.key.clone()).or_default().1.push(c.clone());
    }
    let mut claims = vec![];
    let mut comparisons = 0usize;
    for (key, (a, b)) in grouped {
        if a == b {
            continue;
        }
        let mut taken = BTreeSet::new();
        let mut equivalent = a.len() == b.len();
        for first in &a {
            let mut found = None;
            for (i, second) in b.iter().enumerate() {
                comparisons += 1;
                if comparisons > limits.max_comparisons {
                    return Err(invalid("claim diff comparison budget exceeded"));
                }
                if !taken.contains(&i)
                    && relation(&first.value, &second.value) == ValueRelation::Equivalent
                {
                    found = Some(i);
                    break;
                }
            }
            if let Some(i) = found {
                taken.insert(i);
            } else {
                equivalent = false;
            }
        }
        claims.push(ClaimChange {
            key,
            before: a,
            after: b,
            semantically_equal: equivalent,
        });
    }
    let hash = |r: &ExtractionRecord| {
        r.digests
            .iter()
            .find(|d| d.algorithm == DigestAlgorithm::Sha256)
            .map(|d| d.bytes.clone())
    };
    let content_changed = hash(before).zip(hash(after)).map(|(a, b)| {
        a != b || before.source.identity.size_bytes != after.source.identity.size_bytes
    });
    let out = RecordDiff {
        before_path: before.source.path.clone(),
        after_path: after.source.path.clone(),
        content_changed,
        fields,
        claims,
    };
    if out.fields.len().saturating_add(out.claims.len()) > limits.max_results {
        return Err(invalid("record diff result budget exceeded"));
    }
    serde_json::to_writer(
        crate::LimitedWriter::new(io::sink(), limits.max_bytes),
        &out,
    )
    .map_err(io::Error::other)?;
    Ok(out)
}
pub fn compare_scans(
    before: &[ExtractionRecord],
    after: &[ExtractionRecord],
    limits: AnalysisLimits,
) -> io::Result<ScanDiff> {
    fn index(
        records: &[ExtractionRecord],
        limits: AnalysisLimits,
    ) -> io::Result<BTreeMap<String, &ExtractionRecord>> {
        if records.len() > limits.max_records {
            return Err(invalid("scan diff input limit exceeded"));
        }
        let mut map = BTreeMap::new();
        for r in records {
            check_record(r, limits)?;
            if map.insert(r.source.path.to_string(), r).is_some() {
                return Err(invalid("duplicate path in scan diff"));
            }
        }
        Ok(map)
    }
    let old = index(before, limits)?;
    let new = index(after, limits)?;
    let mut out = ScanDiff {
        added: vec![],
        removed: vec![],
        changed: vec![],
        relocations: vec![],
    };
    let mut removed = BTreeMap::new();
    let mut added = BTreeMap::new();
    for (path, a) in &old {
        if let Some(b) = new.get(path) {
            let diff = compare_records(a, b, limits)?;
            if !diff.fields.is_empty() || !diff.claims.is_empty() {
                out.changed.push(diff);
            }
        } else {
            removed.insert(path.clone(), *a);
        }
    }
    for (path, b) in &new {
        if !old.contains_key(path) {
            added.insert(path.clone(), *b);
        }
    }
    let digest = |r: &ExtractionRecord| {
        r.digests
            .iter()
            .find(|d| d.algorithm == DigestAlgorithm::Sha256 && d.bytes.len() == 32)
            .map(|d| (hex(&d.bytes), r.source.identity.size_bytes))
    };
    let mut old_hashes: BTreeMap<_, Vec<String>> = BTreeMap::new();
    let mut new_hashes: BTreeMap<_, Vec<String>> = BTreeMap::new();
    for (path, r) in &removed {
        if let Some(h) = digest(r) {
            old_hashes.entry(h).or_default().push(path.clone());
        }
    }
    for (path, r) in &added {
        if let Some(h) = digest(r) {
            new_hashes.entry(h).or_default().push(path.clone());
        }
    }
    for (key, paths) in old_hashes {
        if paths.len() == 1 {
            if let Some(new_paths) = new_hashes.get(&key) {
                if new_paths.len() == 1 {
                    let a = removed.remove(&paths[0]).expect("indexed removed path");
                    let b = added.remove(&new_paths[0]).expect("indexed added path");
                    out.relocations.push(Relocation {
                        before: a.source.path.clone(),
                        after: b.source.path.clone(),
                        sha256: key.0,
                    });
                    let diff = compare_records(a, b, limits)?;
                    if !diff.fields.is_empty() || !diff.claims.is_empty() {
                        out.changed.push(diff);
                    }
                }
            }
        }
    }
    out.added = added.into_values().map(|r| r.source.path.clone()).collect();
    out.removed = removed
        .into_values()
        .map(|r| r.source.path.clone())
        .collect();
    if out.added.len() + out.removed.len() + out.changed.len() + out.relocations.len()
        > limits.max_results
    {
        return Err(invalid("scan diff result budget exceeded"));
    }
    serde_json::to_writer(
        crate::LimitedWriter::new(io::sink(), limits.max_bytes),
        &out,
    )
    .map_err(io::Error::other)?;
    Ok(out)
}
