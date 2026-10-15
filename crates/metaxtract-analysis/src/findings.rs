use crate::{
    AnalysisLimits, check_record, invalid,
    normalize::{field, raw, raw_text},
    schema::*,
    validation,
};
use std::{collections::BTreeSet, io};

#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub enum Severity {
    High,
    Medium,
    Low,
    Information,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Category {
    Privacy,
    Contradiction,
    TamperingIndicator,
    AnalysisGap,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Finding {
    pub rule: String,
    pub category: Category,
    pub severity: Severity,
    pub path: EvidencePath,
    pub explanation: String,
    pub observations: Vec<ObservationId>,
    pub claims: Vec<ClaimId>,
    pub heuristic: bool,
}
pub fn classify(record: &ExtractionRecord, limits: AnalysisLimits) -> io::Result<Vec<Finding>> {
    check_record(record, limits)?;
    let mut out = Vec::new();
    let mut add = |rule: &str,
                   category,
                   severity,
                   explanation: &str,
                   observations: Vec<ObservationId>,
                   claims: Vec<ClaimId>,
                   heuristic| {
        out.push(Finding {
            rule: rule.into(),
            category,
            severity,
            path: record.source.path.clone(),
            explanation: explanation.into(),
            observations,
            claims,
            heuristic,
        });
    };
    for c in &record.claims {
        if c.resolution == ClaimResolution::Rejected {
            continue;
        }
        let key = c.key.as_str();
        if matches!(&c.value,NormalizedValue::GeoPoint(p) if p.is_valid()) {
            add(
                "privacy.location",
                Category::Privacy,
                Severity::High,
                "Geographic coordinates can disclose a person's location.",
                c.supporting_observations.clone(),
                vec![c.id],
                false,
            );
        } else if ["author", "last_editor", "device_serial", "lens_serial"]
            .iter()
            .any(|s| key.ends_with(&format!(".{s}")))
            && matches!(&c.value,NormalizedValue::Text(v) if !v.is_empty())
        {
            add(
                "privacy.identity",
                Category::Privacy,
                Severity::Medium,
                "Personal or device identity remains in metadata.",
                c.supporting_observations.clone(),
                vec![c.id],
                false,
            );
        }
        if let NormalizedValue::GeoPoint(p) = c.value {
            if !p.is_valid() {
                add(
                    "anomaly.invalid_location",
                    Category::TamperingIndicator,
                    Severity::Medium,
                    "Coordinate values are outside their valid range; this alone does not establish tampering.",
                    c.supporting_observations.clone(),
                    vec![c.id],
                    true,
                );
            }
        }
        if let NormalizedValue::Timestamp(t) = &c.value {
            if t.nanosecond.is_some_and(|v| v >= 1_000_000_000)
                || t.utc_offset_minutes
                    .is_some_and(|v| !(-1439..=1439).contains(&v))
            {
                add(
                    "anomaly.invalid_time",
                    Category::TamperingIndicator,
                    Severity::Medium,
                    "Timestamp representation contains invalid precision or offset data.",
                    c.supporting_observations.clone(),
                    vec![c.id],
                    true,
                );
            }
        }
    }
    let checked = validation::reconcile(record, limits)?;
    for conflict in checked.conflicts {
        let evidence = record
            .claims
            .iter()
            .filter(|c| conflict.alternatives.contains(&c.id))
            .flat_map(|c| c.supporting_observations.iter().copied())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        add(
            "validation.conflicting_claims",
            Category::Contradiction,
            Severity::Medium,
            &conflict.reason,
            evidence,
            conflict.alternatives,
            false,
        );
    }
    for o in &record.observations {
        let name = field(o);
        let value = raw(o);
        let text = raw_text(value).unwrap_or("");
        if matches!(
            name.as_str(),
            "author"
                | "artist"
                | "lastmodifiedby"
                | "last_modified_by"
                | "body_serial_number"
                | "lens_serial_number"
                | "from"
                | "to"
                | "cc"
                | "bcc"
                | "reply-to"
        ) && !text.trim().is_empty()
        {
            add(
                "privacy.raw_identity",
                Category::Privacy,
                Severity::Medium,
                "An original metadata field contains personal, contact, or device identity.",
                vec![o.id],
                vec![],
                false,
            );
        }
        if text.contains("file://")
            || text.contains("/Users/")
            || text.contains("/home/")
            || text.contains("\\Users\\")
        {
            add(
                "privacy.local_path",
                Category::Privacy,
                Severity::Medium,
                "Metadata discloses a local filesystem location.",
                vec![o.id],
                vec![],
                false,
            );
        }
        if matches!(
            name.as_str(),
            "javascript" | "js" | "embedded_javascript" | "launch_action" | "vba_project" | "macro"
        ) && !matches!(value, RawValue::Null | RawValue::Bool(false))
        {
            add(
                "indicator.active_content",
                Category::TamperingIndicator,
                Severity::Medium,
                "Active content was identified; legitimate documents may also contain it.",
                vec![o.id],
                vec![],
                true,
            );
        }
        if let RawValue::Text(t) = value {
            if !t.lossless {
                add(
                    "coverage.lossy_decoding",
                    Category::AnalysisGap,
                    Severity::Low,
                    "Decoded text is lossy; consult the preserved original bytes.",
                    vec![o.id],
                    vec![],
                    false,
                );
            }
        }
    }
    let created: Vec<_> = record
        .claims
        .iter()
        .filter(|c| {
            c.key.as_str().ends_with("metadata.created")
                && c.resolution != ClaimResolution::Rejected
        })
        .collect();
    let modified: Vec<_> = record
        .claims
        .iter()
        .filter(|c| {
            c.key.as_str().ends_with("metadata.modified")
                && c.resolution != ClaimResolution::Rejected
        })
        .collect();
    let mut comparisons = 0usize;
    for a in created {
        for b in &modified {
            comparisons = comparisons.saturating_add(1);
            if comparisons > limits.max_comparisons {
                return Err(invalid("chronology comparison budget exceeded"));
            }
            if a.key.as_str().strip_suffix("created") != b.key.as_str().strip_suffix("modified") {
                continue;
            }
            if let (NormalizedValue::Timestamp(a_time), NormalizedValue::Timestamp(b_time)) =
                (&a.value, &b.value)
            {
                if a_time
                    .unix_seconds
                    .zip(b_time.unix_seconds)
                    .is_some_and(|(a, b)| b.saturating_add(60) < a)
                {
                    add(
                        "anomaly.chronology",
                        Category::TamperingIndicator,
                        Severity::Low,
                        "Modification precedes creation by more than a minute; clock settings and metadata copying are possible explanations.",
                        a.supporting_observations
                            .iter()
                            .chain(&b.supporting_observations)
                            .copied()
                            .collect(),
                        vec![a.id, b.id],
                        true,
                    );
                }
            }
        }
    }
    if record
        .source
        .declared_media_type
        .as_ref()
        .zip(record.source.detected_media_type.as_ref())
        .is_some_and(|(a, b)| !a.eq_ignore_ascii_case(b))
    {
        add(
            "anomaly.type_mismatch",
            Category::TamperingIndicator,
            Severity::Low,
            "Declared media type differs from detected content; review parser diagnostics and packaging context.",
            vec![],
            vec![],
            true,
        );
    }
    if record.status != ExtractionStatus::Complete || !record.diagnostics.is_empty() {
        add(
            "coverage.incomplete",
            Category::AnalysisGap,
            Severity::Information,
            "Extraction status or diagnostics indicate coverage limits; absence of a finding does not establish absence of metadata.",
            vec![],
            vec![],
            false,
        );
    }
    if out.len() > limits.max_results {
        return Err(invalid("finding result budget exceeded"));
    }
    out.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then(a.rule.cmp(&b.rule))
            .then(a.observations.cmp(&b.observations))
    });
    Ok(out)
}
