use super::{
    EvidenceHashes, ExecutionContext, ExtractionBackend, ExtractionBatch, OpenedEvidence,
    ScanOptions, codec, walk::hex,
};
use crate::schema::{EvidencePath, ExtractionRecord, ExtractionStatus};
use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
pub fn engine_fingerprint() -> &'static str {
    env!("METAXTRACT_ENGINE_FINGERPRINT")
}
pub struct RecordCache {
    conn: Mutex<rusqlite::Connection>,
    path: PathBuf,
    max_bytes: u64,
    max_entries: u64,
}
impl RecordCache {
    pub fn open(path: impl AsRef<Path>, max_bytes: u64, max_entries: u64) -> io::Result<Arc<Self>> {
        if max_bytes == 0
            || max_bytes > i64::MAX as u64
            || max_entries == 0
            || max_entries > i64::MAX as u64
        {
            return Err(io::Error::other("invalid cache limits"));
        }
        let conn = codec::database(path.as_ref())?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS scan_cache(cache_key TEXT PRIMARY KEY,data BLOB NOT NULL,digest BLOB NOT NULL,accessed INTEGER NOT NULL);").map_err(io::Error::other)?;
        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            path: path.as_ref().canonicalize()?,
            max_bytes,
            max_entries,
        }))
    }
    fn get(&self, key: &str, max: usize) -> io::Result<Option<Vec<ExtractionRecord>>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| io::Error::other("cache lock poisoned"))?;
        let row: Option<(Vec<u8>, Vec<u8>)> = conn
            .query_row(
                "SELECT data,digest FROM scan_cache WHERE cache_key=?1 AND length(data)<=?2",
                params![key, max as i64],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(io::Error::other)?;
        let Some((data, digest)) = row else {
            return Ok(None);
        };
        let records = if Sha256::digest(&data).as_slice() == digest {
            codec::decode(&data, max).ok()
        } else {
            None
        };
        if records.is_some() {
            conn.execute(
                "UPDATE scan_cache SET accessed=unixepoch() WHERE cache_key=?1",
                [key],
            )
            .map_err(io::Error::other)?;
        } else {
            conn.execute("DELETE FROM scan_cache WHERE cache_key=?1", [key])
                .map_err(io::Error::other)?;
        }
        Ok(records)
    }
    fn put(&self, key: &str, records: &[ExtractionRecord], max: usize) -> io::Result<()> {
        let data = codec::encode(&records, max)?;
        if data.len() as u64 > self.max_bytes {
            return Ok(());
        }
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| io::Error::other("cache lock poisoned"))?;
        let tx = conn.transaction().map_err(io::Error::other)?;
        tx.execute("INSERT OR REPLACE INTO scan_cache(cache_key,data,digest,accessed) VALUES(?1,?2,?3,unixepoch())",params![key,&data,Sha256::digest(&data).to_vec()]).map_err(io::Error::other)?;
        loop {
            let (count, bytes): (i64, i64) = tx
                .query_row(
                    "SELECT count(*),coalesce(sum(length(data)),0) FROM scan_cache",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(io::Error::other)?;
            if count <= self.max_entries as i64 && bytes <= self.max_bytes as i64 {
                break;
            }
            tx.execute("DELETE FROM scan_cache WHERE cache_key=(SELECT cache_key FROM scan_cache ORDER BY accessed,cache_key LIMIT 1)",[]).map_err(io::Error::other)?;
        }
        tx.commit().map_err(io::Error::other)
    }
}
pub struct CachedExtractor {
    inner: Arc<dyn ExtractionBackend>,
    cache: Arc<RecordCache>,
}
impl CachedExtractor {
    pub fn new(inner: Arc<dyn ExtractionBackend>, cache: Arc<RecordCache>) -> Arc<Self> {
        Arc::new(Self { inner, cache })
    }
}
impl ExtractionBackend for CachedExtractor {
    fn fingerprint(&self) -> String {
        self.inner.fingerprint()
    }
    fn excluded_paths(&self) -> Vec<PathBuf> {
        let mut paths = self.inner.excluded_paths();
        paths.extend(database_artifacts(&self.cache.path));
        paths
    }
    fn extract(
        &self,
        evidence: &OpenedEvidence,
        hashes: &EvidenceHashes,
        options: &ScanOptions,
        context: &ExecutionContext,
    ) -> io::Result<ExtractionBatch> {
        context.checkpoint()?;
        let max =
            (options.max_result_bytes.saturating_mul(4)).min(codec::MAX_BLOB_BYTES as u64) as usize;
        let bytes = codec::encode(
            &(
                crate::SCHEMA_REVISION,
                metaxtract_native::NATIVE_API_REVISION,
                engine_fingerprint(),
                self.inner.fingerprint(),
                &hashes.digests,
                evidence.relative.name_bytes()?,
                options,
            ),
            16 * 1024 * 1024,
        )?;
        let key = hex(&Sha256::digest(bytes));
        if hashes.stable {
            if let Ok(Some(mut records)) = self.cache.get(&key, max) {
                for record in &mut records {
                    rebase(record, &evidence.evidence_path);
                }
                if let Some(root) = records.first_mut() {
                    root.source.identity = evidence.stamp.identity();
                }
                context.checkpoint()?;
                return Ok(ExtractionBatch {
                    records,
                    cache_hit: true,
                });
            }
        }
        let result = self.inner.extract(evidence, hashes, options, context)?;
        if hashes.stable
            && super::FileStamp::read(&evidence.file)? == hashes.after
            && !result.records.is_empty()
            && result.records.iter().all(|r| {
                matches!(
                    r.status,
                    ExtractionStatus::Complete | ExtractionStatus::Unsupported
                )
            })
        {
            let _ = self.cache.put(&key, &result.records, max);
        }
        Ok(result)
    }
}
fn rebase(record: &mut ExtractionRecord, root: &crate::schema::RelativePath) {
    fn path(value: &EvidencePath, root: &crate::schema::RelativePath) -> EvidencePath {
        let mut new = EvidencePath::root(root.clone());
        for s in value.nested() {
            new.push(s.clone());
        }
        new
    }
    record.source.path = path(&record.source.path, root);
    for o in &mut record.observations {
        o.provenance.source = path(&o.provenance.source, root);
    }
    for d in &mut record.diagnostics {
        d.source = path(&d.source, root);
    }
}
pub(crate) fn database_artifacts(path: &Path) -> Vec<PathBuf> {
    let mut paths = vec![path.to_owned()];
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut raw = path.as_os_str().to_os_string();
        raw.push(suffix);
        paths.push(raw.into());
    }
    paths
}
