use super::{
    EvidenceHashes, FileStamp, HashOptions, NativePath, OpenedEvidence, ScanRoot, WalkEntry,
    WalkOptions, hash_evidence, walk::invalid,
};
use crate::{
    RecursiveLimits, extract_file_recursive,
    schema::{
        Diagnostic, DiagnosticSeverity, DiagnosticStage, EvidencePath, ExtractionRecord,
        ExtractionStatus, SourceDescriptor, SourceKind,
    },
};
use std::{
    io,
    sync::{
        Arc, Mutex,
        mpsc::{self, SyncSender, TrySendError},
    },
    thread,
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct ScanOptions {
    pub walk: WalkOptions,
    pub hash: HashOptions,
    pub recursive: RecursiveLimits,
    pub workers: usize,
    pub queue_capacity: usize,
}
impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            walk: WalkOptions::default(),
            hash: HashOptions::default(),
            recursive: RecursiveLimits::default(),
            workers: thread::available_parallelism().map_or(2, |n| n.get().min(4)),
            queue_capacity: 8,
        }
    }
}
impl ScanOptions {
    pub(crate) fn validate(&self) -> io::Result<()> {
        self.hash.validate()?;
        if !(1..=32).contains(&self.workers) || !(1..=256).contains(&self.queue_capacity) {
            return Err(invalid("invalid scan concurrency"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct FileTask {
    pub id: u64,
    pub relative: NativePath,
    pub evidence_path: crate::schema::RelativePath,
    pub stamp: FileStamp,
}
#[derive(Clone, Debug)]
pub struct ScannedFile {
    pub task: FileTask,
    pub hashes: Option<EvidenceHashes>,
    pub records: Vec<ExtractionRecord>,
    pub error: Option<String>,
    pub unstable: bool,
    pub cache_hit: bool,
}
#[derive(Clone, Debug, Default)]
pub struct ScanSummary {
    pub discovered: u64,
    pub completed: u64,
    pub failed: u64,
    pub unstable: u64,
    pub notices: u64,
    pub bytes_hashed: u64,
}
#[derive(Clone, Debug)]
pub enum ScanEvent {
    Discovered(FileTask),
    Started(FileTask),
    Completed(Box<ScannedFile>),
    Notice {
        path: Option<NativePath>,
        code: String,
        message: String,
    },
    DiscoveryFinished,
    Finished(ScanSummary),
}
pub struct ExtractionBatch {
    pub records: Vec<ExtractionRecord>,
    pub cache_hit: bool,
}
pub trait ExtractionBackend: Send + Sync {
    fn fingerprint(&self) -> String;
    fn excluded_paths(&self) -> Vec<std::path::PathBuf> {
        Vec::new()
    }
    fn extract(
        &self,
        evidence: &OpenedEvidence,
        hashes: &EvidenceHashes,
        options: &ScanOptions,
    ) -> io::Result<ExtractionBatch>;
}
pub struct NativeExtractor;
impl ExtractionBackend for NativeExtractor {
    fn fingerprint(&self) -> String {
        format!(
            "native:{}:{}",
            crate::SCHEMA_REVISION,
            metaxtract_native::NATIVE_API_REVISION
        )
    }
    fn extract(
        &self,
        evidence: &OpenedEvidence,
        hashes: &EvidenceHashes,
        options: &ScanOptions,
    ) -> io::Result<ExtractionBatch> {
        let tree = extract_file_recursive(
            evidence.file.try_clone()?,
            hashes.prefix.clone(),
            evidence.relative.name_bytes()?,
            options.recursive,
        )
        .map_err(|e| io::Error::other(e.to_string()))?;
        let records = tree
            .canonical_records(evidence.evidence_path.clone())
            .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(ExtractionBatch {
            records,
            cache_hit: false,
        })
    }
}
pub fn scan_stream(
    root: Arc<ScanRoot>,
    mut options: ScanOptions,
    backend: Arc<dyn ExtractionBackend>,
    mut sink: impl FnMut(ScanEvent) -> io::Result<()>,
) -> io::Result<ScanSummary> {
    options.validate()?;
    for path in backend.excluded_paths() {
        if let Some(relative) = root.relative_to_root(&path) {
            options.walk.excluded.push(relative);
        }
    }
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    let (jobs_tx, jobs_rx) = mpsc::sync_channel(options.queue_capacity);
    let jobs_rx = Arc::new(Mutex::new(jobs_rx));
    let (events_tx, events_rx) = mpsc::sync_channel(options.queue_capacity);
    let mut summary = ScanSummary::default();
    thread::scope(|scope| -> io::Result<()> {
        let tx = events_tx.clone();
        let opts = &options;
        let root = &root;
        let producer_cancelled = &cancelled;
        scope.spawn(move || {
            let walk = match root.walk(opts.walk.clone()) {
                Ok(w) => w,
                Err(e) => {
                    let _ = tx.send(ScanEvent::Notice {
                        path: None,
                        code: "walk.root".into(),
                        message: e.to_string(),
                    });
                    return;
                }
            };
            let mut next = 0_u64;
            for entry in walk {
                if producer_cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                match entry {
                    WalkEntry::File(file) => {
                        next += 1;
                        let task = FileTask {
                            id: next,
                            relative: file.relative.clone(),
                            evidence_path: file.evidence_path.clone(),
                            stamp: file.stamp.clone(),
                        };
                        if tx.send(ScanEvent::Discovered(task.clone())).is_err()
                            || !send_job(&jobs_tx, (task, file), producer_cancelled)
                        {
                            return;
                        }
                    }
                    WalkEntry::Notice {
                        path,
                        code,
                        message,
                    } => {
                        if tx
                            .send(ScanEvent::Notice {
                                path,
                                code,
                                message,
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
            let _ = tx.send(ScanEvent::DiscoveryFinished);
        });
        for _ in 0..options.workers {
            let rx = Arc::clone(&jobs_rx);
            let tx = events_tx.clone();
            let backend = &backend;
            let opts = &options;
            let worker_cancelled = &cancelled;
            scope.spawn(move || {
                loop {
                    if worker_cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    let work = rx
                        .lock()
                        .map_err(|_| ())
                        .and_then(|r| r.recv().map_err(|_| ()));
                    let Ok((task, mut file)) = work else {
                        break;
                    };
                    if tx.send(ScanEvent::Started(task.clone())).is_err() {
                        break;
                    }
                    let result = process(task, &mut file, opts, backend.as_ref());
                    if tx.send(ScanEvent::Completed(Box::new(result))).is_err() {
                        break;
                    }
                }
            });
        }
        drop(events_tx);
        for event in events_rx {
            match &event {
                ScanEvent::Discovered(_) => summary.discovered += 1,
                ScanEvent::Completed(file) => {
                    summary.completed += 1;
                    summary.failed += u64::from(file.error.is_some());
                    summary.unstable += u64::from(file.unstable);
                    summary.bytes_hashed = summary
                        .bytes_hashed
                        .saturating_add(file.hashes.as_ref().map_or(0, |h| h.bytes_read));
                }
                ScanEvent::Notice { .. } => summary.notices += 1,
                _ => {}
            }
            if let Err(e) = sink(event) {
                cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                return Err(e);
            }
        }
        Ok(())
    })?;
    sink(ScanEvent::Finished(summary.clone()))?;
    Ok(summary)
}
fn send_job<T>(
    tx: &SyncSender<T>,
    mut value: T,
    cancelled: &std::sync::atomic::AtomicBool,
) -> bool {
    loop {
        if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            return false;
        }
        match tx.try_send(value) {
            Ok(()) => return true,
            Err(TrySendError::Disconnected(_)) => return false,
            Err(TrySendError::Full(v)) => {
                value = v;
                thread::sleep(Duration::from_millis(5));
            }
        }
    }
}
fn process(
    task: FileTask,
    file: &mut OpenedEvidence,
    options: &ScanOptions,
    backend: &dyn ExtractionBackend,
) -> ScannedFile {
    let mut result = ScannedFile {
        task,
        hashes: None,
        records: Vec::new(),
        error: None,
        unstable: false,
        cache_hit: false,
    };
    let work = (|| -> io::Result<()> {
        let hashes = hash_evidence(file, &options.hash, || Ok(()))?;
        result.unstable = !hashes.stable;
        result.hashes = Some(hashes);
        if result.unstable {
            return Err(io::Error::other("file changed during hashing"));
        }
        let batch = backend.extract(
            file,
            result.hashes.as_ref().expect("hash result exists"),
            options,
        )?;
        result.records = batch.records;
        result.cache_hit = batch.cache_hit;
        if result.records.is_empty() {
            return Err(io::Error::other("extractor returned no root record"));
        }
        if FileStamp::read(&file.file)? != file.stamp {
            result.unstable = true;
            return Err(io::Error::other("file changed during extraction"));
        }
        Ok(())
    })();
    if let Err(e) = work {
        result.error = Some(e.to_string());
    }
    finalize(&mut result);
    result
}
pub(crate) fn finalize(result: &mut ScannedFile) {
    if result.records.is_empty() {
        result.records.push(ExtractionRecord::new(SourceDescriptor {
            path: EvidencePath::root(result.task.evidence_path.clone()),
            kind: SourceKind::File,
            identity: result.task.stamp.identity(),
            declared_media_type: None,
            detected_media_type: None,
        }));
    }
    let record = &mut result.records[0];
    record.source.identity = result.task.stamp.identity();
    record.digests = result
        .hashes
        .as_ref()
        .map_or_else(Vec::new, |h| h.digests.clone());
    if let Some(message) = &result.error {
        record.status = if record.observations.is_empty() {
            ExtractionStatus::Failed
        } else {
            ExtractionStatus::Partial
        };
        record.diagnostics.push(Diagnostic {
            code: if result.unstable {
                "scan.changed"
            } else {
                "scan.failed"
            }
            .into(),
            severity: DiagnosticSeverity::Error,
            stage: DiagnosticStage::Integrity,
            message: message.clone(),
            parser: None,
            source: record.source.path.clone(),
            source_range: None,
            related_observations: Vec::new(),
            recoverable: true,
        });
    }
}
