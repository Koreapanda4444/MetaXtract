use crate::{AnalysisLimits, check_record, hex, invalid, normalize::field, schema::*};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Selector {
    ClaimPrefix(String),
    RawNamespacePrefix(String),
    RawName(String),
    Privacy,
    AllMetadata,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RedactionAction {
    Remove,
    Mask(String),
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RedactionRule {
    pub selector: Selector,
    pub action: RedactionAction,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RedactionProfile {
    pub name: String,
    pub rules: Vec<RedactionRule>,
    pub strip_encoded_payloads: bool,
    pub omit_unstructured_context: bool,
    pub hide_paths: bool,
    pub omit_digests: bool,
    pub omit_filesystem_times: bool,
}
impl RedactionProfile {
    pub fn privacy() -> Self {
        Self {
            name: "privacy".into(),
            rules: vec![RedactionRule {
                selector: Selector::Privacy,
                action: RedactionAction::Remove,
            }],
            strip_encoded_payloads: true,
            omit_unstructured_context: true,
            hide_paths: false,
            omit_digests: false,
            omit_filesystem_times: false,
        }
    }
    pub fn validate(&self) -> io::Result<()> {
        if self.name.is_empty()
            || self.name.len() > 256
            || self.name.chars().any(char::is_control)
            || self.rules.len() > 128
        {
            return Err(invalid("invalid profile name or rule count"));
        }
        for rule in &self.rules {
            let s = match &rule.selector {
                Selector::ClaimPrefix(v) => {
                    CanonicalKey::parse(v.clone()).map_err(io::Error::other)?;
                    Some(v)
                }
                Selector::RawNamespacePrefix(v) | Selector::RawName(v) => Some(v),
                _ => None,
            };
            if s.is_some_and(|v| v.is_empty() || v.len() > 4096 || v.chars().any(char::is_control))
            {
                return Err(invalid("invalid redaction selector"));
            }
            if matches!(&rule.action,RedactionAction::Mask(v) if v.len()>4096||v.contains('\0')) {
                return Err(invalid("invalid redaction mask"));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RedactionChange {
    pub observation: ObservationId,
    pub action: RedactionAction,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RedactionPreview {
    pub profile: String,
    pub original_record_sha256: Option<String>,
    pub record: ExtractionRecord,
    pub changes: Vec<RedactionChange>,
    pub removed_claims: Vec<ClaimId>,
    pub unselected_observations: usize,
    pub stripped_payload_observations: Vec<ObservationId>,
    pub derivative: bool,
}
fn privacy_name(name: &str) -> bool {
    name.contains("latitude")
        || name.contains("longitude")
        || name.contains("altitude")
        || name.contains("serial")
        || matches!(
            name,
            "author"
                | "creator"
                | "artist"
                | "lastmodifiedby"
                | "last_modified_by"
                | "from"
                | "to"
                | "cc"
                | "bcc"
                | "reply-to"
                | "email"
                | "owner"
        )
}
fn selected_claim(selector: &Selector, c: &NormalizedClaim) -> bool {
    match selector {
        Selector::AllMetadata => true,
        Selector::ClaimPrefix(p) => {
            c.key.as_str() == p
                || c.key
                    .as_str()
                    .strip_prefix(p)
                    .is_some_and(|s| s.starts_with('.'))
        }
        Selector::Privacy => {
            matches!(c.value, NormalizedValue::GeoPoint(_))
                || c.key.as_str().split('.').any(privacy_name)
        }
        _ => false,
    }
}
fn selected_observation(selector: &Selector, o: &MetadataObservation) -> bool {
    match selector {
        Selector::AllMetadata => true,
        Selector::RawNamespacePrefix(p) => o.tag.namespace.starts_with(p),
        Selector::RawName(n) => field(o) == n.to_ascii_lowercase(),
        Selector::Privacy => privacy_name(&field(o)) || o.tag.namespace.ends_with(".gps"),
        _ => false,
    }
}
fn merge<K: Ord>(
    map: &mut BTreeMap<K, RedactionAction>,
    key: K,
    action: &RedactionAction,
) -> io::Result<bool> {
    if let Some(old) = map.get(&key) {
        if old != action {
            return Err(invalid(
                "overlapping redaction rules specify incompatible actions",
            ));
        }
        return Ok(false);
    }
    map.insert(key, action.clone());
    Ok(true)
}
fn strip_payloads(value: &mut RawValue) -> bool {
    match value {
        RawValue::Bytes(_) => {
            *value = RawValue::Null;
            true
        }
        RawValue::Text(t) => t.encoded_bytes.take().is_some(),
        RawValue::Sequence(v) => {
            let mut changed = false;
            for v in v {
                changed |= strip_payloads(v);
            }
            changed
        }
        RawValue::Structure(v) => {
            let mut changed = false;
            for v in v {
                changed |= strip_payloads(&mut v.value);
            }
            changed
        }
        _ => false,
    }
}
/// Preview a metadata derivative. Original evidence bytes and the workspace are never modified.
pub fn preview(
    record: &ExtractionRecord,
    profile: &RedactionProfile,
    path_alias: Option<EvidencePath>,
    limits: AnalysisLimits,
) -> io::Result<RedactionPreview> {
    check_record(record, limits)?;
    profile.validate()?;
    if profile.hide_paths && path_alias.is_none() {
        return Err(invalid(
            "path hiding requires an explicit non-sensitive alias",
        ));
    }
    let mut observations = BTreeMap::new();
    let mut keys = BTreeMap::new();
    let mut work = 0usize;
    for rule in &profile.rules {
        for o in &record.observations {
            work += 1;
            if work > limits.max_comparisons {
                return Err(invalid("redaction comparison budget exceeded"));
            }
            if selected_observation(&rule.selector, o) {
                merge(&mut observations, o.id, &rule.action)?;
            }
        }
        for c in &record.claims {
            work += 1;
            if work > limits.max_comparisons {
                return Err(invalid("redaction comparison budget exceeded"));
            }
            if selected_claim(&rule.selector, c) {
                merge(&mut keys, c.key.clone(), &rule.action)?;
            }
        }
    }
    loop {
        let mut changed = false;
        for claim in &record.claims {
            work += 1;
            if work > limits.max_comparisons {
                return Err(invalid("redaction dependency budget exceeded"));
            }
            let linked: Vec<_> = claim
                .supporting_observations
                .iter()
                .chain(&claim.contradicting_observations)
                .filter_map(|id| observations.get(id))
                .collect();
            for (i, action) in linked.iter().enumerate() {
                if i > 0 && *action != linked[0] {
                    return Err(invalid(
                        "linked metadata has incompatible redaction actions",
                    ));
                }
            }
            let action = keys
                .get(&claim.key)
                .or_else(|| linked.first().copied())
                .cloned();
            if let Some(action) = action {
                changed |= merge(&mut keys, claim.key.clone(), &action)?;
                for id in claim
                    .supporting_observations
                    .iter()
                    .chain(&claim.contradicting_observations)
                {
                    changed |= merge(&mut observations, *id, &action)?;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut derived = record.clone();
    let mut changes = vec![];
    let mut stripped_payload_observations = vec![];
    derived.observations.retain_mut(|o| {
        if let Some(action) = observations.get(&o.id) {
            changes.push(RedactionChange {
                observation: o.id,
                action: action.clone(),
            });
            match action {
                RedactionAction::Remove => return false,
                RedactionAction::Mask(mask) => {
                    o.raw_value = RawValue::Text(TextValue {
                        text: mask.clone(),
                        original_encoding: Some("UTF-8".into()),
                        language: None,
                        encoded_bytes: None,
                        lossless: true,
                    });
                    o.interpreted_value = None;
                    o.unit = None;
                    o.provenance.parser = ParserDescriptor {
                        id: "metaxtract.redaction".into(),
                        name: "MetaXtract redaction derivative".into(),
                        version: "1".into(),
                        library: None,
                    };
                    o.provenance.method = ExtractionMethod::Heuristic;
                }
            }
        }
        if profile.strip_encoded_payloads {
            let mut stripped = strip_payloads(&mut o.raw_value);
            if let Some(v) = &mut o.interpreted_value {
                stripped |= strip_payloads(v);
            }
            if stripped {
                stripped_payload_observations.push(o.id);
            }
        }
        true
    });
    let removed_claims: Vec<_> = derived
        .claims
        .iter()
        .filter(|c| keys.contains_key(&c.key))
        .map(|c| c.id)
        .collect();
    derived.claims.retain(|c| !keys.contains_key(&c.key));
    let kept: BTreeSet<_> = derived.claims.iter().map(|c| c.id).collect();
    derived.conflicts.retain_mut(|c| {
        c.alternatives.retain(|id| kept.contains(id));
        c.alternatives.len() > 1
    });
    if profile.omit_unstructured_context {
        derived.diagnostics.clear();
        derived.containers.clear();
        derived.conflicts.clear();
        derived.source.identity.stable_id = None;
    }
    let remaining: BTreeSet<_> = derived.observations.iter().map(|o| o.id).collect();
    for d in &mut derived.diagnostics {
        d.related_observations.retain(|id| remaining.contains(id));
    }
    if profile.hide_paths {
        derived.source.path = path_alias.expect("validated alias");
        derived.source.identity.stable_id = None;
        for o in &mut derived.observations {
            o.provenance.source = derived.source.path.clone();
        }
        for d in &mut derived.diagnostics {
            d.source = derived.source.path.clone();
        }
    }
    if profile.omit_digests {
        derived.digests.clear();
    }
    if profile.omit_filesystem_times {
        derived.source.identity.created_ns = None;
        derived.source.identity.modified_ns = None;
    }
    derived.status = ExtractionStatus::Partial;
    derived.diagnostics.push(Diagnostic{code:"redaction.derivative".into(),severity:DiagnosticSeverity::Information,stage:DiagnosticStage::Integrity,message:"Field-selected metadata derivative; original evidence bytes were not sanitized. Unselected metadata requires review.".into(),parser:None,source:derived.source.path.clone(),source_range:None,related_observations:vec![],recoverable:true});
    let original_record_sha256 = if profile.omit_digests {
        None
    } else {
        Some(hex(&Sha256::digest(
            serde_json::to_vec(record).map_err(io::Error::other)?,
        )))
    };
    let changed: BTreeSet<_> = changes
        .iter()
        .map(|c| c.observation)
        .chain(stripped_payload_observations.iter().copied())
        .collect();
    let unselected_observations = record.observations.len().saturating_sub(changed.len());
    let out = RedactionPreview {
        profile: profile.name.clone(),
        original_record_sha256,
        record: derived,
        changes,
        removed_claims,
        unselected_observations,
        stripped_payload_observations,
        derivative: true,
    };
    check_record(&out.record, limits)?;
    serde_json::to_writer(
        crate::LimitedWriter::new(io::sink(), limits.max_bytes),
        &out,
    )
    .map_err(io::Error::other)?;
    Ok(out)
}
