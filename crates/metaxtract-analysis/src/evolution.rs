use crate::{
    AnalysisLimits, check_record,
    diff::{self, RecordDiff},
    invalid,
    schema::*,
};
use std::{collections::BTreeSet, io};
pub struct ScanSnapshot {
    pub scan_id: String,
    pub started_ms: i64,
    pub record: Option<ExtractionRecord>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ScanMoment {
    pub scan_id: String,
    pub started_ms: i64,
    pub present: bool,
}
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum Presence {
    Both,
    MissingBefore,
    MissingAfter,
    MissingBoth,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Transition {
    pub before: usize,
    pub after: usize,
    pub presence: Presence,
    pub changes: Option<RecordDiff>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Evolution {
    pub path: EvidencePath,
    pub scans: Vec<ScanMoment>,
    pub transitions: Vec<Transition>,
}
/// Tracks a specified evidence path. Absence means no record in that scan, not proof of deletion.
pub fn history(
    path: &EvidencePath,
    mut snapshots: Vec<ScanSnapshot>,
    limits: AnalysisLimits,
) -> io::Result<Evolution> {
    if snapshots.len() > limits.max_records || snapshots.len() < 2 {
        return Err(invalid(
            "history requires two or more scans within the record budget",
        ));
    }
    snapshots.sort_by(|a, b| (a.started_ms, &a.scan_id).cmp(&(b.started_ms, &b.scan_id)));
    let mut ids = BTreeSet::new();
    let mut bytes = 0usize;
    for s in &snapshots {
        if !ids.insert(&s.scan_id) || s.scan_id.is_empty() || s.scan_id.len() > 256 {
            return Err(invalid("history scan IDs must be distinct and nonempty"));
        }
        if let Some(r) = &s.record {
            check_record(r, limits)?;
            if &r.source.path != path {
                return Err(invalid("history record differs from tracked path"));
            }
            bytes = bytes.saturating_add(crate::encoded_size(r, limits.max_bytes)?);
            if bytes > limits.max_bytes {
                return Err(invalid("history input byte budget exceeded"));
            }
        }
    }
    let scans = snapshots
        .iter()
        .map(|s| ScanMoment {
            scan_id: s.scan_id.clone(),
            started_ms: s.started_ms,
            present: s.record.is_some(),
        })
        .collect();
    let mut transitions = vec![];
    let mut results = 0usize;
    for (i, pair) in snapshots.windows(2).enumerate() {
        let (presence, changes) = match (&pair[0].record, &pair[1].record) {
            (Some(a), Some(b)) => (
                Presence::Both,
                Some(diff::compare_records(
                    a,
                    b,
                    AnalysisLimits {
                        max_comparisons: limits.max_comparisons / (snapshots.len() - 1),
                        ..limits
                    },
                )?),
            ),
            (None, Some(_)) => (Presence::MissingBefore, None),
            (Some(_), None) => (Presence::MissingAfter, None),
            (None, None) => (Presence::MissingBoth, None),
        };
        results = results.saturating_add(
            changes
                .as_ref()
                .map(|c| c.fields.len() + c.claims.len())
                .unwrap_or(1),
        );
        if results > limits.max_results {
            return Err(invalid("history change budget exceeded"));
        }
        transitions.push(Transition {
            before: i,
            after: i + 1,
            presence,
            changes,
        });
    }
    let out = Evolution {
        path: path.clone(),
        scans,
        transitions,
    };
    crate::encoded_size(&out, limits.max_bytes)?;
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> ExtractionRecord {
        ExtractionRecord::new(SourceDescriptor {
            path: EvidencePath::root(RelativePath::parse("a").unwrap()),
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
    fn history_orders_scans_and_keeps_missing_evidence_explicit() {
        let r = record();
        let path = r.source.path.clone();
        let h = history(
            &path,
            vec![
                ScanSnapshot {
                    scan_id: "later".into(),
                    started_ms: 3,
                    record: Some(r.clone()),
                },
                ScanSnapshot {
                    scan_id: "first".into(),
                    started_ms: 1,
                    record: Some(r),
                },
                ScanSnapshot {
                    scan_id: "gap".into(),
                    started_ms: 2,
                    record: None,
                },
            ],
            Default::default(),
        )
        .unwrap();
        assert_eq!(h.scans[0].scan_id, "first");
        assert_eq!(h.transitions[0].presence, Presence::MissingAfter);
        assert_eq!(h.transitions[1].presence, Presence::MissingBefore);
    }
    #[test]
    fn changed_fields_and_limits_are_retained() {
        let a = record();
        let mut b = a.clone();
        b.status = ExtractionStatus::Partial;
        let path = a.source.path.clone();
        let snapshots = || {
            vec![
                ScanSnapshot {
                    scan_id: "a".into(),
                    started_ms: 1,
                    record: Some(a.clone()),
                },
                ScanSnapshot {
                    scan_id: "b".into(),
                    started_ms: 2,
                    record: Some(b.clone()),
                },
            ]
        };
        let h = history(&path, snapshots(), Default::default()).unwrap();
        assert_eq!(h.transitions[0].changes.as_ref().unwrap().fields.len(), 1);
        assert!(
            history(
                &path,
                snapshots(),
                AnalysisLimits {
                    max_results: 0,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
}
