use crate::{AnalysisLimits, check_record, invalid, schema::*};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ValueRelation {
    Equivalent,
    Contradictory,
    Indeterminate,
}
pub fn relation(a: &NormalizedValue, b: &NormalizedValue) -> ValueRelation {
    use ValueRelation::*;
    match (a, b) {
        (NormalizedValue::Timestamp(a), NormalizedValue::Timestamp(b)) => {
            if a == b {
                return Equivalent;
            }
            match (a.unix_seconds, b.unix_seconds) {
                (Some(x), Some(y)) => {
                    let first =
                        i128::from(x) * 1_000_000_000 + i128::from(a.nanosecond.unwrap_or(0));
                    let second =
                        i128::from(y) * 1_000_000_000 + i128::from(b.nanosecond.unwrap_or(0));
                    let width = |t: &TimestampValue| match t.precision {
                        TimestampPrecision::Minute => 60_000_000_000i128,
                        TimestampPrecision::Second => 1_000_000_000,
                        TimestampPrecision::FractionalSecond => {
                            let digits = t
                                .original
                                .split_once('.')
                                .map(|(_, v)| v.bytes().take_while(u8::is_ascii_digit).count())
                                .unwrap_or(9)
                                .min(9);
                            10i128.pow((9 - digits) as u32)
                        }
                        _ => i128::MAX / 4,
                    };
                    if first == second && width(a) == width(b) {
                        Equivalent
                    } else if first.saturating_add(width(a)) <= second
                        || second.saturating_add(width(b)) <= first
                    {
                        Contradictory
                    } else {
                        Indeterminate
                    }
                }
                _ => Indeterminate,
            }
        }
        (NormalizedValue::Number(x), NormalizedValue::Number(y))
            if !x.is_finite() || !y.is_finite() =>
        {
            Indeterminate
        }
        (NormalizedValue::GeoPoint(x), NormalizedValue::GeoPoint(y))
            if !x.is_valid() || !y.is_valid() =>
        {
            Indeterminate
        }
        (NormalizedValue::Signed(x), NormalizedValue::Unsigned(y))
        | (NormalizedValue::Unsigned(y), NormalizedValue::Signed(x)) => {
            if u64::try_from(*x).ok() == Some(*y) {
                Equivalent
            } else {
                Contradictory
            }
        }
        _ => {
            if a == b {
                Equivalent
            } else {
                Contradictory
            }
        }
    }
}
pub fn parser_family(parser: &ParserDescriptor) -> String {
    parser
        .library
        .as_ref()
        .map(|l| format!("library:{}", l.name.to_lowercase()))
        .unwrap_or_else(|| {
            format!(
                "builtin:{}",
                parser.id.split('.').next().unwrap_or("").to_lowercase()
            )
        })
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ValidationIssue {
    pub claim: ClaimId,
    pub reason: String,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Validation {
    pub claims: Vec<NormalizedClaim>,
    pub conflicts: Vec<ClaimConflict>,
    pub issues: Vec<ValidationIssue>,
}
pub fn reconcile(record: &ExtractionRecord, limits: AnalysisLimits) -> io::Result<Validation> {
    check_record(record, limits)?;
    let mut groups: BTreeMap<&CanonicalKey, Vec<&NormalizedClaim>> = BTreeMap::new();
    for claim in &record.claims {
        groups.entry(&claim.key).or_default().push(claim);
    }
    let obs: BTreeMap<_, _> = record.observations.iter().map(|o| (o.id, o)).collect();
    let mut out = Validation {
        claims: vec![],
        conflicts: vec![],
        issues: vec![],
    };
    let mut comparisons = 0usize;
    for (key, claims) in groups {
        let mut disputed = BTreeSet::new();
        for claim in &claims {
            if claim.resolution == ClaimResolution::Rejected {
                out.claims.push((*claim).clone());
                continue;
            }
            let mut supported = BTreeSet::new();
            let mut dissent = BTreeSet::new();
            let mut uncertain = false;
            for other in &claims {
                comparisons = comparisons.saturating_add(1);
                if comparisons > limits.max_comparisons {
                    return Err(invalid("claim comparison budget exceeded"));
                }
                if other.resolution == ClaimResolution::Rejected {
                    continue;
                }
                match relation(&claim.value, &other.value) {
                    ValueRelation::Equivalent => {
                        supported.extend(other.supporting_observations.iter().copied())
                    }
                    ValueRelation::Contradictory => {
                        dissent.extend(other.supporting_observations.iter().copied());
                        disputed.extend([claim.id, other.id]);
                    }
                    ValueRelation::Indeterminate => uncertain = true,
                }
            }
            dissent.extend(claim.contradicting_observations.iter().copied());
            let overlap = supported.intersection(&dissent).next().is_some();
            let mut families = BTreeSet::new();
            let mut parsers = BTreeSet::new();
            let mut dissenters = BTreeSet::new();
            for id in &supported {
                let o = obs[id];
                families.insert(parser_family(&o.provenance.parser));
                parsers.insert(o.provenance.parser.id.clone());
            }
            for id in &dissent {
                dissenters.insert(obs[id].provenance.parser.id.clone());
            }
            let mut c = (*claim).clone();
            c.supporting_observations = supported.into_iter().collect();
            c.contradicting_observations = dissent.into_iter().collect();
            c.supporting_parsers = parsers.into_iter().collect();
            c.dissenting_parsers = dissenters.into_iter().collect();
            let conflict = !c.contradicting_observations.is_empty() || disputed.contains(&claim.id);
            c.agreement = if conflict {
                AgreementLevel::Disputed
            } else if families.len() >= 2 {
                AgreementLevel::Corroborated
            } else {
                AgreementLevel::SingleParser
            };
            let mut basis = vec![];
            if !c.supporting_observations.is_empty() {
                basis.push(ConfidenceBasis::DirectTag);
            }
            if c.agreement == AgreementLevel::Corroborated {
                basis.push(ConfidenceBasis::IndependentParserAgreement);
            }
            c.confidence = Confidence::new(
                if conflict {
                    400
                } else if families.len() >= 2 {
                    950
                } else if families.len() == 1 {
                    800
                } else {
                    0
                },
                basis,
            )
            .ok_or_else(|| invalid("invalid derived confidence"))?;
            if !matches!(
                c.resolution,
                ClaimResolution::Rejected | ClaimResolution::UserSelected
            ) {
                c.resolution = if !conflict && !uncertain && !c.supporting_observations.is_empty() {
                    ClaimResolution::Accepted
                } else {
                    ClaimResolution::Unresolved
                };
            }
            if c.supporting_observations.is_empty() || uncertain || overlap {
                out.issues.push(ValidationIssue {
                    claim: claim.id,
                    reason: if overlap {
                        "one observation supports incompatible interpretations"
                    } else if uncertain {
                        "precision or missing timezone prevents a decisive comparison"
                    } else {
                        "claim has no linked observation; parser names alone are not evidence"
                    }
                    .into(),
                });
            }
            out.claims.push(c);
        }
        if !disputed.is_empty() {
            out.conflicts.push(ClaimConflict{key:key.clone(),alternatives:disputed.into_iter().collect(),reason:"incompatible interpretations of the same canonical field; retain all alternatives".into()});
        }
    }
    out.claims.sort_by_key(|c| c.id);
    if out
        .claims
        .len()
        .saturating_add(out.conflicts.len())
        .saturating_add(out.issues.len())
        > limits.max_results
    {
        return Err(invalid("validation result budget exceeded"));
    }
    Ok(out)
}
