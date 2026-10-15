use crate::{AnalysisLimits, check_record, invalid, schema::*, timestamp};
use std::{collections::BTreeMap, io};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ObservationLink {
    pub observation: ObservationId,
    pub tag: TagIdentity,
    pub provenance: Provenance,
    pub contradicts: bool,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FieldLineage {
    pub path: EvidencePath,
    pub claim: ClaimId,
    pub key: CanonicalKey,
    pub value: NormalizedValue,
    pub evidence: Vec<ObservationLink>,
    pub confidence: Confidence,
    pub agreement: AgreementLevel,
    pub resolution: ClaimResolution,
    pub conflicts: Vec<ClaimConflict>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum TimelineOrigin {
    Claim(Box<FieldLineage>),
    FileSystem { field: String },
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TimelineEvent {
    pub path: EvidencePath,
    pub event: String,
    pub timestamp: TimestampValue,
    pub origin: TimelineOrigin,
}
pub fn field_lineage(
    record: &ExtractionRecord,
    limits: AnalysisLimits,
) -> io::Result<Vec<FieldLineage>> {
    check_record(record, limits)?;
    let observations: BTreeMap<_, _> = record.observations.iter().map(|o| (o.id, o)).collect();
    let claims: BTreeMap<_, _> = record.claims.iter().map(|c| (c.id, c)).collect();
    let mut conflicts: BTreeMap<_, Vec<ClaimConflict>> = BTreeMap::new();
    for conflict in &record.conflicts {
        for id in &conflict.alternatives {
            if claims.get(id).is_none_or(|c| c.key != conflict.key) {
                return Err(invalid("conflict references a missing or different field"));
            }
            conflicts.entry(*id).or_default().push(conflict.clone());
        }
    }
    let mut out = Vec::new();
    let mut links = 0usize;
    for c in &record.claims {
        let mut evidence = Vec::new();
        for (ids, contradicts) in [
            (&c.supporting_observations, false),
            (&c.contradicting_observations, true),
        ] {
            for id in ids {
                links = links.saturating_add(1);
                if links > limits.max_results {
                    return Err(invalid("lineage link budget exceeded"));
                }
                let o = observations
                    .get(id)
                    .ok_or_else(|| invalid("missing lineage observation"))?;
                evidence.push(ObservationLink {
                    observation: *id,
                    tag: o.tag.clone(),
                    provenance: o.provenance.clone(),
                    contradicts,
                });
            }
        }
        out.push(FieldLineage {
            path: record.source.path.clone(),
            claim: c.id,
            key: c.key.clone(),
            value: c.value.clone(),
            evidence,
            confidence: c.confidence.clone(),
            agreement: c.agreement,
            resolution: c.resolution,
            conflicts: conflicts.remove(&c.id).unwrap_or_default(),
        });
        if out.len() > limits.max_results {
            return Err(invalid("lineage result budget exceeded"));
        }
    }
    Ok(out)
}
pub fn timeline(
    records: &[ExtractionRecord],
    limits: AnalysisLimits,
) -> io::Result<Vec<TimelineEvent>> {
    if records.len() > limits.max_records {
        return Err(invalid("timeline record budget exceeded"));
    }
    let mut out = Vec::new();
    for record in records {
        for lineage in field_lineage(record, limits)? {
            if let NormalizedValue::Timestamp(time) = &lineage.value {
                out.push(TimelineEvent {
                    path: record.source.path.clone(),
                    event: lineage.key.to_string(),
                    timestamp: time.clone(),
                    origin: TimelineOrigin::Claim(Box::new(lineage)),
                });
            }
        }
        for (field, value) in [
            ("filesystem.created", record.source.identity.created_ns),
            ("filesystem.modified", record.source.identity.modified_ns),
        ] {
            if let Some(value) = value {
                let time = timestamp::from_unix_nanoseconds(value)
                    .ok_or_else(|| invalid("filesystem time exceeds supported epoch range"))?;
                out.push(TimelineEvent {
                    path: record.source.path.clone(),
                    event: field.into(),
                    timestamp: time,
                    origin: TimelineOrigin::FileSystem {
                        field: field.into(),
                    },
                });
            }
        }
        if out.len() > limits.max_results {
            return Err(invalid("timeline result budget exceeded"));
        }
    }
    out.sort_by(|a, b| {
        match (a.timestamp.unix_seconds, b.timestamp.unix_seconds) {
            (Some(x), Some(y)) => x.cmp(&y).then(
                a.timestamp
                    .nanosecond
                    .unwrap_or(0)
                    .cmp(&b.timestamp.nanosecond.unwrap_or(0)),
            ),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.timestamp.original.cmp(&b.timestamp.original),
        }
        .then_with(|| a.path.to_string().cmp(&b.path.to_string()))
        .then(a.event.cmp(&b.event))
    });
    Ok(out)
}
