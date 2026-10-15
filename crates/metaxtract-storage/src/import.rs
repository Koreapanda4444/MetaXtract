mod bundle;
mod legacy;
mod python;

use crate::{
    CaseWorkspace, Result, codec, db, hex, invalid, now_ms, records::insert_record, transport,
};
use metaxtract_schema::{DigestAlgorithm, ExtractionRecord};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
};

#[derive(Clone, Debug)]
pub struct ImportLimits {
    pub max_input_bytes: u64,
    pub max_member_bytes: u64,
    pub max_total_bytes: u64,
    pub max_members: usize,
    pub max_records: u64,
    pub max_record_bytes: usize,
    pub max_control_bytes: usize,
}
impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 2 * 1024 * 1024 * 1024,
            max_member_bytes: 1024 * 1024 * 1024,
            max_total_bytes: 4 * 1024 * 1024 * 1024,
            max_members: 10_000,
            max_records: 1_000_000,
            max_record_bytes: codec::MAX_RECORD_BYTES,
            max_control_bytes: 32 * 1024 * 1024,
        }
    }
}
impl ImportLimits {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.max_input_bytes == 0
            || self.max_member_bytes == 0
            || self.max_total_bytes == 0
            || self.max_records == 0
            || self.max_records > i64::MAX as u64
            || self.max_members == 0
            || self.max_members > 60_000
            || self.max_record_bytes == 0
            || self.max_record_bytes > codec::MAX_RECORD_BYTES
            || self.max_control_bytes == 0
            || self.max_control_bytes > codec::MAX_RECORD_BYTES
        {
            return Err(invalid("invalid import resource limits"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportSummary {
    pub import_id: i64,
    pub scans: Vec<String>,
    pub records: u64,
    pub signed: bool,
    pub snapshot_sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportReceipt {
    pub id: i64,
    pub kind: String,
    pub source: String,
    pub snapshot_sha256: String,
    pub imported_ms: i64,
    pub details: Value,
}

impl CaseWorkspace {
    pub fn scan_receipt(&self, scan: &str, key: &str) -> Result<Option<Value>> {
        crate::text(scan, 1024, false)?;
        crate::text(key, 1024, false)?;
        let row: Option<(Vec<u8>, Vec<u8>)> = self
            .lock()?
            .query_row(
                "SELECT data,digest FROM scan_receipts WHERE scan_id=?1 AND key=?2",
                params![scan, key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        row.map(|(data, digest)| {
            if Sha256::digest(&data).as_slice() != digest {
                return Err(invalid("scan receipt checksum mismatch"));
            }
            codec::decode(&data, 16 * 1024 * 1024)
        })
        .transpose()
    }
    pub fn import_receipt(&self, id: i64) -> Result<Option<ImportReceipt>> {
        let conn = self.lock()?;
        let row: Option<(String, String, String, i64, Vec<u8>)> = conn
            .query_row(
                "SELECT kind,source,snapshot_sha256,imported_ms,details FROM imports WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()
            .map_err(db)?;
        row.map(|(kind, source, snapshot_sha256, imported_ms, details)| {
            Ok(ImportReceipt {
                id,
                kind,
                source,
                snapshot_sha256,
                imported_ms,
                details: codec::decode(&details, 32 * 1024 * 1024)?,
            })
        })
        .transpose()
    }
    pub fn import_artifact(&self, id: i64, name: &str) -> Result<Option<Vec<u8>>> {
        let row: Option<(Vec<u8>, Vec<u8>)> = self
            .lock()?
            .query_row(
                "SELECT data,digest FROM import_artifacts WHERE import_id=?1 AND path=?2",
                params![id, name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        row.map(|(data, digest)| {
            if data.len() > codec::MAX_RECORD_BYTES || Sha256::digest(&data).as_slice() != digest {
                return Err(invalid("import artifact checksum differs"));
            }
            Ok(data)
        })
        .transpose()
    }
    pub fn import_jsonl(
        &self,
        path: impl AsRef<Path>,
        limits: &ImportLimits,
    ) -> Result<ImportSummary> {
        limits.validate()?;
        let path = path.as_ref();
        let (snapshot, hash) = transport::snapshot(path, limits.max_input_bytes)?;
        self.write_named("import_jsonl", |tx| {
            let scan = new_scan(tx, &path.to_string_lossy(), "import_jsonl")?;
            let (records, _) = read_records(
                tx,
                &scan,
                &mut BufReader::new(File::open(snapshot.path())?),
                limits,
                None,
            )?;
            tx.execute(
                "UPDATE scans SET status='completed',finished_ms=?2 WHERE id=?1",
                params![scan, now_ms()?],
            )
            .map_err(db)?;
            let import_id = receipt(
                tx,
                "jsonl",
                path,
                &hash,
                &json!({"source_path":native_path(path)}),
            )?;
            Ok(ImportSummary {
                import_id,
                scans: vec![scan],
                records,
                signed: false,
                snapshot_sha256: hash.clone(),
            })
        })
    }
    pub fn original_import_record(&self, id: i64) -> Result<Option<Vec<u8>>> {
        let row: Option<(Vec<u8>, Vec<u8>)> = self
            .lock()?
            .query_row(
                "SELECT original,digest FROM legacy_records WHERE record_id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        row.map(|(data, digest)| {
            if data.len() > codec::MAX_RECORD_BYTES || Sha256::digest(&data).as_slice() != digest {
                return Err(invalid("original imported record checksum differs"));
            }
            Ok(data)
        })
        .transpose()
    }
    pub fn append_records_with_receipt(
        &self,
        scan: &str,
        records: &[ExtractionRecord],
        key: &str,
        receipt: &impl Serialize,
    ) -> Result<Vec<i64>> {
        crate::text(key, 1024, false)?;
        let data = codec::encode(receipt, 16 * 1024 * 1024)?;
        self.write_named("append_records_with_receipt", |tx| {
            let ids = records
                .iter()
                .map(|r| insert_record(tx, scan, r))
                .collect::<Result<Vec<_>>>()?;
            tx.execute(
                "INSERT INTO scan_receipts VALUES(?1,?2,?3,?4)",
                params![scan, key, &data, Sha256::digest(&data).to_vec()],
            )
            .map_err(db)?;
            Ok(ids)
        })
    }
}

pub(crate) fn native_path(path: &Path) -> Value {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        json!({"display":path.to_string_lossy(),"unix_bytes":path.as_os_str().as_bytes()})
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        json!({"display":path.to_string_lossy(),"windows_utf16":path.as_os_str().encode_wide().collect::<Vec<_>>()})
    }
    #[cfg(not(any(unix, windows)))]
    {
        json!({"display":path.to_string_lossy()})
    }
}
fn new_scan(tx: &Transaction<'_>, source: &str, origin: &str) -> Result<String> {
    let id: String = tx
        .query_row("SELECT lower(hex(randomblob(16)))", [], |r| r.get(0))
        .map_err(db)?;
    tx.execute("INSERT INTO scans(id,source,origin,status,started_ms,imported_from) VALUES(?1,?2,?3,'running',?4,?2)",params![id,source,origin,now_ms()?]).map_err(db)?;
    Ok(id)
}
fn receipt(
    tx: &Transaction<'_>,
    kind: &str,
    path: &Path,
    hash: &str,
    details: &Value,
) -> Result<i64> {
    tx.execute("INSERT INTO imports(kind,source,snapshot_sha256,imported_ms,details) VALUES(?1,?2,?3,?4,?5)",params![kind,path.to_string_lossy(),hash,now_ms()?,codec::encode(details,32*1024*1024)?]).map_err(db)?;
    Ok(tx.last_insert_rowid())
}
fn artifact(tx: &Transaction<'_>, import: i64, path: &str, data: &[u8]) -> Result<()> {
    tx.execute(
        "INSERT INTO import_artifacts VALUES(?1,?2,?3,?4)",
        params![import, path, data, Sha256::digest(data).to_vec()],
    )
    .map_err(db)?;
    Ok(())
}
fn read_records(
    tx: &Transaction<'_>,
    scan: &str,
    reader: &mut impl BufRead,
    limits: &ImportLimits,
    manifest: Option<&Value>,
) -> Result<(u64, String)> {
    let mut count = 0u64;
    let mut first = true;
    let mut hash = Sha256::new();
    let originals: std::collections::BTreeMap<_, _> = manifest
        .into_iter()
        .flat_map(|m| {
            m.get("original_files")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|v| v.get("path").and_then(Value::as_str).map(|p| (p, v)))
        .collect();
    loop {
        let mut line = Vec::new();
        let read = reader
            .by_ref()
            .take(limits.max_record_bytes as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        if line.len() > limits.max_record_bytes {
            return Err(invalid("JSONL record exceeds byte budget"));
        }
        let input = if first && line.starts_with(&[0xef, 0xbb, 0xbf]) {
            &line[3..]
        } else {
            &line
        };
        first = false;
        if input.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if count >= limits.max_records {
            return Err(invalid("import exceeds record count budget"));
        }
        let value: Value = serde_json::from_slice(input).map_err(std::io::Error::other)?;
        let record: ExtractionRecord =
            if value.get("source").is_some() && value.get("observations").is_some() {
                serde_json::from_slice(input).map_err(std::io::Error::other)?
            } else {
                legacy::convert(&value)?
            };
        let digest = record
            .digests
            .iter()
            .find(|d| d.algorithm == DigestAlgorithm::Sha256)
            .map(|d| hex(&d.bytes))
            .unwrap_or_default();
        if let Some(manifest) = manifest {
            let expected = manifest
                .get("hashes")
                .and_then(Value::as_array)
                .and_then(|v| v.get(count as usize))
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("manifest record hash is missing"))?;
            if expected != digest {
                return Err(invalid("manifest record hash differs"));
            }
            if let Some(original) = originals.get(record.source.path.root_path().as_str()) {
                if original.get("sha256").and_then(Value::as_str) != Some(digest.as_str())
                    || original.get("size_bytes").and_then(Value::as_u64)
                        != Some(record.source.identity.size_bytes)
                {
                    return Err(invalid(
                        "original evidence declaration differs from scan record",
                    ));
                }
            }
        }
        hash.update(format!(
            "{digest}\t{}\n",
            record.source.path.root_path().as_str()
        ));
        let id = insert_record(tx, scan, &record)?;
        tx.execute(
            "INSERT INTO legacy_records VALUES(?1,?2,?3)",
            params![id, &line, Sha256::digest(&line).to_vec()],
        )
        .map_err(db)?;
        count += 1;
    }
    if count == 0 {
        hash.update(b"\n");
    }
    if let Some(manifest) = manifest {
        if manifest.get("record_count").and_then(Value::as_u64) != Some(count)
            || manifest
                .get("hashes")
                .and_then(Value::as_array)
                .map(Vec::len)
                != Some(count as usize)
        {
            return Err(invalid("manifest record count differs"));
        }
    }
    Ok((count, hex(&hash.finalize())))
}
