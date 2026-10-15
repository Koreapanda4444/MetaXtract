use crate::{CaseWorkspace, Result, codec, db, hex, invalid, now_ms, text};
use metaxtract_schema::{DiagnosticSeverity, DigestAlgorithm, ExtractionRecord, NormalizedValue};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum ScanStatus {
    Running,
    Completed,
    Paused,
    Cancelled,
    Failed,
}
impl ScanStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Paused => "paused",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "paused" => Ok(Self::Paused),
            "cancelled" => Ok(Self::Cancelled),
            "failed" => Ok(Self::Failed),
            _ => Err(invalid("invalid case scan status")),
        }
    }
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MetadataCounts {
    pub observations: u64,
    pub claims: u64,
    pub conflicts: u64,
    pub diagnostics: u64,
}

impl CaseWorkspace {
    pub fn begin_scan(&self, source: &str, origin: &str) -> Result<String> {
        text(source, 1024 * 1024, false)?;
        text(origin, 256, false)?;
        self.write(|tx|{
            let id:String=tx.query_row("SELECT lower(hex(randomblob(16)))",[],|r|r.get(0)).map_err(db)?;
            tx.execute("INSERT INTO scans(id,source,origin,status,started_ms) VALUES(?1,?2,?3,'running',?4)",params![id,source,origin,now_ms()?]).map_err(db)?;Ok(id)
        })
    }
    pub fn finish_scan(&self, id: &str, status: ScanStatus) -> Result<()> {
        if status == ScanStatus::Running {
            return Err(invalid("a final scan status is required"));
        }
        self.write(|tx|{
            if tx.execute("UPDATE scans SET status=?2,finished_ms=?3 WHERE id=?1 AND status IN('running','paused')",params![id,status.as_str(),now_ms()?]).map_err(db)?!=1{return Err(invalid("unknown or finalized scan"));}Ok(())
        })
    }
    pub fn append_record(&self, scan: &str, record: &ExtractionRecord) -> Result<i64> {
        self.write(|tx| insert_record(tx, scan, record))
    }
    pub fn append_records(&self, scan: &str, records: &[ExtractionRecord]) -> Result<Vec<i64>> {
        self.write(|tx| records.iter().map(|r| insert_record(tx, scan, r)).collect())
    }
    pub fn record(&self, id: i64) -> Result<Option<ExtractionRecord>> {
        let conn = self.lock()?;
        let row: Option<(Vec<u8>, Vec<u8>)> = conn
            .query_row(
                "SELECT data,digest FROM records WHERE id=?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        row.map(|(data, digest)| {
            if data.len() > codec::MAX_RECORD_BYTES || Sha256::digest(&data).as_slice() != digest {
                return Err(invalid("case record checksum mismatch"));
            }
            let record = codec::decode(&data, codec::MAX_RECORD_BYTES)?;
            validate(&record)?;
            Ok(record)
        })
        .transpose()
    }
    pub fn metadata_counts(&self, id: i64) -> Result<MetadataCounts> {
        self.lock()?.query_row("SELECT (SELECT count(*) FROM raw_fields WHERE record_id=?1),(SELECT count(*) FROM normalized_claims WHERE record_id=?1),(SELECT count(*) FROM conflicts WHERE record_id=?1),(SELECT count(*) FROM diagnostics WHERE record_id=?1)",[id],|r|Ok(MetadataCounts{observations:r.get(0)?,claims:r.get(1)?,conflicts:r.get(2)?,diagnostics:r.get(3)?})).map_err(db)
    }
}

pub(crate) fn insert_record(
    tx: &Transaction<'_>,
    scan: &str,
    record: &ExtractionRecord,
) -> Result<i64> {
    validate(record)?;
    let state: String = tx
        .query_row("SELECT status FROM scans WHERE id=?1", [scan], |r| r.get(0))
        .map_err(db)?;
    if state != "running" {
        return Err(invalid("records can only be appended to a running scan"));
    }
    let data = codec::encode(record, codec::MAX_RECORD_BYTES)?;
    let ordinal: i64 = tx
        .query_row("SELECT record_count FROM scans WHERE id=?1", [scan], |r| {
            r.get(0)
        })
        .map_err(db)?;
    let hash = record
        .digests
        .iter()
        .find(|d| d.algorithm == DigestAlgorithm::Sha256)
        .map(|d| hex(&d.bytes));
    tx.execute("INSERT INTO records(scan_id,ordinal,path,status,mime,size_bytes,size_key,sha256,data,digest) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![scan,ordinal,record.source.path.to_string(),format!("{:?}",record.status),record.source.detected_media_type.as_deref().or(record.source.declared_media_type.as_deref()).unwrap_or("application/octet-stream"),record.source.identity.size_bytes.to_string(),record.source.identity.size_bytes.to_be_bytes().as_slice(),hash,&data,Sha256::digest(&data).to_vec()]).map_err(db)?;
    let id = tx.last_insert_rowid();
    for (i, observation) in record.observations.iter().enumerate() {
        let data = codec::encode(observation, codec::MAX_RECORD_BYTES)?;
        tx.execute(
            "INSERT INTO raw_fields VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                id,
                i as i64,
                observation.id.get().to_string(),
                observation.tag.namespace,
                serde_json::to_string(&observation.tag.code).map_err(std::io::Error::other)?,
                &data,
                Sha256::digest(&data).to_vec()
            ],
        )
        .map_err(db)?;
    }
    for (i, claim) in record.claims.iter().enumerate() {
        let data = codec::encode(claim, codec::MAX_RECORD_BYTES)?;
        let value = match &claim.value {
            NormalizedValue::Text(v) => Some(v.clone()),
            NormalizedValue::Signed(v) => Some(v.to_string()),
            NormalizedValue::Unsigned(v) => Some(v.to_string()),
            NormalizedValue::Bool(v) => Some(v.to_string()),
            _ => None,
        };
        tx.execute(
            "INSERT INTO normalized_claims VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                id,
                i as i64,
                claim.id.get().to_string(),
                claim.key.as_str(),
                value,
                &data,
                Sha256::digest(&data).to_vec()
            ],
        )
        .map_err(db)?;
    }
    for (i, conflict) in record.conflicts.iter().enumerate() {
        let data = codec::encode(conflict, codec::MAX_RECORD_BYTES)?;
        tx.execute(
            "INSERT INTO conflicts VALUES(?1,?2,?3,?4,?5)",
            params![
                id,
                i as i64,
                conflict.key.as_str(),
                &data,
                Sha256::digest(&data).to_vec()
            ],
        )
        .map_err(db)?;
    }
    for (i, diagnostic) in record.diagnostics.iter().enumerate() {
        let data = codec::encode(diagnostic, codec::MAX_RECORD_BYTES)?;
        tx.execute(
            "INSERT INTO diagnostics VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                id,
                i as i64,
                diagnostic.code,
                format!("{:?}", diagnostic.severity),
                &data,
                Sha256::digest(&data).to_vec()
            ],
        )
        .map_err(db)?;
    }
    let warnings = record
        .diagnostics
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Warning)
        .count() as i64;
    let errors = record
        .diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.severity,
                DiagnosticSeverity::Error | DiagnosticSeverity::Fatal
            )
        })
        .count() as i64;
    tx.execute("UPDATE scans SET record_count=record_count+1,warning_count=warning_count+?2,error_count=error_count+?3 WHERE id=?1",params![scan,warnings,errors]).map_err(db)?;
    Ok(id)
}

pub(crate) fn validate(record: &ExtractionRecord) -> Result<()> {
    let observations: BTreeSet<_> = record.observations.iter().map(|r| r.id.get()).collect();
    let claims: BTreeMap<_, _> = record
        .claims
        .iter()
        .map(|r| (r.id.get(), r.key.as_str()))
        .collect();
    if observations.len() != record.observations.len() || claims.len() != record.claims.len() {
        return Err(invalid("duplicate metadata identifiers"));
    }
    for claim in &record.claims {
        if claim
            .supporting_observations
            .iter()
            .chain(&claim.contradicting_observations)
            .any(|id| !observations.contains(&id.get()))
        {
            return Err(invalid("claim references an unknown observation"));
        }
    }
    for conflict in &record.conflicts {
        if conflict
            .alternatives
            .iter()
            .any(|id| claims.get(&id.get()) != Some(&conflict.key.as_str()))
        {
            return Err(invalid(
                "conflict references an unknown or differently keyed claim",
            ));
        }
    }
    for diagnostic in &record.diagnostics {
        if diagnostic
            .related_observations
            .iter()
            .any(|id| !observations.contains(&id.get()))
        {
            return Err(invalid("diagnostic references an unknown observation"));
        }
    }
    for digest in &record.digests {
        if matches!(
            digest.algorithm,
            DigestAlgorithm::Sha256 | DigestAlgorithm::Blake3
        ) && digest.bytes.len() != 32
        {
            return Err(invalid("invalid evidence digest length"));
        }
    }
    Ok(())
}
