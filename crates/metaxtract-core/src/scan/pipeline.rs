use super::{
    EvidenceHashes, ExecutionContext, FileStamp, HashOptions, NativePath, OpenedEvidence,
    ScanControl, ScanRoot, StopReason, WalkEntry, WalkOptions, hash_evidence,
    memory::{MemoryBudget, MemoryLease, bound_records},
    walk::invalid,
};
use crate::{
    RecursiveLimits, extract_file_recursive_controlled,
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

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ScanOptions {
    pub walk: WalkOptions,
    pub hash: HashOptions,
    pub recursive: RecursiveLimits,
    pub workers: usize,
    pub queue_capacity: usize,
    pub max_memory_bytes: u64,
    pub max_result_bytes: u64,
    pub file_wall_time_ms: u64,
    pub scan_wall_time_ms: Option<u64>,
}
impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            walk: WalkOptions::default(),
            hash: HashOptions::default(),
            recursive: RecursiveLimits::default(),
            workers: thread::available_parallelism().map_or(2, |n| n.get().min(4)),
            queue_capacity: 8,
            max_memory_bytes: 4 * 1024 * 1024 * 1024,
            max_result_bytes: 256 * 1024 * 1024,
            file_wall_time_ms: 120_000,
            scan_wall_time_ms: None,
        }
    }
}
impl ScanOptions {
    pub(crate) fn validate(&self) -> io::Result<()> {
        self.hash.validate()?;
        if !(1..=32).contains(&self.workers) || !(1..=256).contains(&self.queue_capacity) {
            return Err(invalid("invalid scan concurrency"));
        }
        if self.file_wall_time_ms == 0
            || self.file_wall_time_ms > 86_400_000
            || self
                .scan_wall_time_ms
                .is_some_and(|v| v == 0 || v > 31 * 86_400_000)
            || self.max_result_bytes < 1024 * 1024
            || self.max_result_bytes > 256 * 1024 * 1024
            || self.reservation() > self.max_memory_bytes
        {
            return Err(invalid("invalid scan resource policy"));
        }
        Ok(())
    }
}
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct FileTask {
    pub id: u64,
    pub relative: NativePath,
    pub evidence_path: crate::schema::RelativePath,
    pub stamp: FileStamp,
}
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ScannedFile {
    pub task: FileTask,
    pub hashes: Option<EvidenceHashes>,
    pub records: Vec<ExtractionRecord>,
    pub error: Option<String>,
    pub unstable: bool,
    pub cache_hit: bool,
}
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub struct ScanSummary {
    pub discovered: u64,
    pub completed: u64,
    pub failed: u64,
    pub unstable: u64,
    pub notices: u64,
    pub bytes_hashed: u64,
    pub stopped: Option<StopReason>,
}
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum ScanEvent {
    Discovered(FileTask),
    Resumed(FileTask),
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
        context: &ExecutionContext,
    ) -> io::Result<ExtractionBatch>;
}
pub struct NativeExtractor;
impl ExtractionBackend for NativeExtractor {
    fn fingerprint(&self) -> String {
        super::engine_fingerprint().to_owned()
    }
    fn extract(
        &self,
        evidence: &OpenedEvidence,
        hashes: &EvidenceHashes,
        options: &ScanOptions,
        context: &ExecutionContext,
    ) -> io::Result<ExtractionBatch> {
        context.checkpoint()?;
        let mut limits = options.recursive;
        limits.max_wall_time_ms = limits
            .max_wall_time_ms
            .min(context.remaining_ms().min(u64::from(u32::MAX)) as u32);
        let tree = extract_file_recursive_controlled(
            evidence.file.try_clone()?,
            hashes.prefix.clone(),
            evidence.relative.name_bytes()?,
            limits,
            || context.checkpoint(),
        )
        .map_err(|e| io::Error::other(e.to_string()))?;
        let mut records = tree
            .canonical_records(evidence.evidence_path.clone())
            .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(ExtractionBatch {
            records,
            cache_hit: false,
        })
    }
}
impl ScanOptions {
    pub(crate) fn reservation(&self) -> u64 {
        self.recursive
            .parser
            .max_allocation_bytes
            .saturating_add(self.recursive.parser.max_decoded_bytes.saturating_mul(2))
            .saturating_add(self.recursive.max_total_input_bytes.saturating_mul(2))
            .saturating_add(self.max_result_bytes.saturating_mul(2))
            .saturating_add(self.hash.buffer_bytes as u64)
            .saturating_add(1024 * 1024)
    }
}
impl ScannedFile {
    pub fn failed(&self) -> bool {
        self.error.is_some()
            || self
                .records
                .first()
                .is_some_and(|r| r.status == ExtractionStatus::Failed)
    }
}
pub fn scan_stream(
    root: Arc<ScanRoot>,
    options: ScanOptions,
    backend: Arc<dyn ExtractionBackend>,
    sink: impl FnMut(ScanEvent) -> io::Result<()>,
) -> io::Result<ScanSummary> {
    scan_stream_with_control(root, options, backend, ScanControl::new(), sink)
}
pub fn scan_stream_with_control(
    root: Arc<ScanRoot>,
    mut options: ScanOptions,
    backend: Arc<dyn ExtractionBackend>,
    control: ScanControl,
    sink: impl FnMut(ScanEvent) -> io::Result<()>,
) -> io::Result<ScanSummary> {
    for path in backend.excluded_paths() {
        if let Some(relative) = root.relative_to_root(&path) {
            if !options.walk.excluded.contains(&relative) {
                options.walk.excluded.push(relative);
            }
        }
    }
    run_pipeline(
        root,
        options,
        backend,
        control,
        Arc::new(super::checkpoint::NoJournal),
        sink,
    )
}
pub(crate) fn run_pipeline(
    root: Arc<ScanRoot>,
    options: ScanOptions,
    backend: Arc<dyn ExtractionBackend>,
    control: ScanControl,
    journal: Arc<dyn super::checkpoint::ScanJournal>,
    mut sink: impl FnMut(ScanEvent) -> io::Result<()>,
) -> io::Result<ScanSummary> {
    options.validate()?;
    control.begin(options.scan_wall_time_ms)?;
    let memory = MemoryBudget::new(options.max_memory_bytes);
    let (jobs_tx, jobs_rx) =
        mpsc::sync_channel::<(FileTask, io::Result<OpenedEvidence>)>(options.queue_capacity);
    let jobs_rx = Arc::new(Mutex::new(jobs_rx));
    let (events_tx, events_rx) = mpsc::sync_channel(options.queue_capacity);
    let mut summary = ScanSummary::default();
    thread::scope(|scope| -> io::Result<()> {
        let tx = events_tx.clone();
        let producer_control = control.clone();
        let producer_journal = Arc::clone(&journal);
        let root = &root;
        let opts = &options;
        scope.spawn(move || {
            let produce = (|| -> io::Result<()> {
                let mut after = 0_u64;
                loop {
                    producer_control.checkpoint()?;
                    let pending = producer_journal.pending(after, 64)?;
                    if pending.is_empty() {
                        break;
                    }
                    for task in pending {
                        after = task.id;
                        let file = root.open_file(&task.relative, opts.walk.symlinks);
                        if tx
                            .send(Envelope::Event(ScanEvent::Resumed(task.clone())))
                            .is_err()
                            || !send_job(&jobs_tx, (task, file), &producer_control)
                        {
                            return Ok(());
                        }
                    }
                }
                if producer_journal.discovery_done()? {
                    return Ok(());
                }
                let mut next = 0_u64;
                for entry in root.walk(opts.walk.clone())? {
                    producer_control.checkpoint()?;
                    match entry {
                        WalkEntry::File(file) => {
                            next += 1;
                            let task = FileTask {
                                id: next,
                                relative: file.relative.clone(),
                                evidence_path: file.evidence_path.clone(),
                                stamp: file.stamp.clone(),
                            };
                            if let Some(task) = producer_journal.schedule(task)? {
                                if tx
                                    .send(Envelope::Event(ScanEvent::Discovered(task.clone())))
                                    .is_err()
                                    || !send_job(&jobs_tx, (task, Ok(file)), &producer_control)
                                {
                                    return Ok(());
                                }
                            }
                        }
                        WalkEntry::Notice {
                            path,
                            code,
                            message,
                        } => {
                            if tx
                                .send(Envelope::Event(ScanEvent::Notice {
                                    path,
                                    code,
                                    message,
                                }))
                                .is_err()
                            {
                                return Ok(());
                            }
                        }
                    }
                }
                producer_journal.finish_discovery()?;
                let _ = tx.send(Envelope::Event(ScanEvent::DiscoveryFinished));
                Ok(())
            })();
            if let Err(error) = produce {
                if producer_control.reason().is_none() {
                    let _ = tx.send(Envelope::Event(ScanEvent::Notice {
                        path: None,
                        code: "scan.discovery".into(),
                        message: error.to_string(),
                    }));
                    producer_control.cancel();
                }
            }
        });
        for _ in 0..options.workers {
            let rx = Arc::clone(&jobs_rx);
            let tx = events_tx.clone();
            let backend = &backend;
            let opts = &options;
            let worker_control = control.clone();
            let memory = Arc::clone(&memory);
            let worker_journal = Arc::clone(&journal);
            scope.spawn(move || {
                loop {
                    if worker_control.checkpoint().is_err() {
                        break;
                    }
                    let work = rx
                        .lock()
                        .map_err(|_| ())
                        .and_then(|r| r.recv().map_err(|_| ()));
                    let Ok((task, file)) = work else {
                        break;
                    };
                    let Ok(lease) = memory.acquire(opts.reservation(), &worker_control) else {
                        break;
                    };
                    if let Err(error) = worker_journal.started(&task) {
                        let _ = tx.send(Envelope::Event(ScanEvent::Notice {
                            path: Some(task.relative.clone()),
                            code: "scan.checkpoint".into(),
                            message: error.to_string(),
                        }));
                        worker_control.cancel();
                        break;
                    }
                    if tx
                        .send(Envelope::Event(ScanEvent::Started(task.clone())))
                        .is_err()
                    {
                        break;
                    }
                    let context = worker_control.context(opts.file_wall_time_ms);
                    let result = match file {
                        Ok(mut file) => process(task, &mut file, opts, backend.as_ref(), &context),
                        Err(error) => {
                            let mut result = ScannedFile {
                                task,
                                hashes: None,
                                records: Vec::new(),
                                error: Some(format!(
                                    "checkpoint source cannot be reopened: {error}"
                                )),
                                unstable: true,
                                cache_hit: false,
                            };
                            finalize(&mut result);
                            result
                        }
                    };
                    if worker_control.reason().is_some() {
                        break;
                    }
                    if tx
                        .send(Envelope::Completed {
                            file: Box::new(result),
                            _lease: lease,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
        drop(events_tx);
        for envelope in events_rx {
            let (event, _lease) = match envelope {
                Envelope::Event(e) => (e, None),
                Envelope::Completed { file, _lease } => (ScanEvent::Completed(file), Some(_lease)),
            };
            match &event {
                ScanEvent::Discovered(_) => summary.discovered += 1,
                ScanEvent::Completed(file) => {
                    summary.completed += 1;
                    summary.failed += u64::from(file.failed());
                    summary.unstable += u64::from(file.unstable);
                    summary.bytes_hashed = summary
                        .bytes_hashed
                        .saturating_add(file.hashes.as_ref().map_or(0, |h| h.bytes_read));
                }
                ScanEvent::Notice { .. } => summary.notices += 1,
                _ => {}
            }
            if let Err(e) = sink(event) {
                control.cancel();
                return Err(e);
            }
        }
        Ok(())
    })?;
    summary.stopped = control.reason();
    sink(ScanEvent::Finished(summary.clone()))?;
    Ok(summary)
}
enum Envelope {
    Event(ScanEvent),
    Completed {
        file: Box<ScannedFile>,
        _lease: MemoryLease,
    },
}
fn send_job<T>(tx: &SyncSender<T>, mut value: T, control: &ScanControl) -> bool {
    loop {
        if control.checkpoint().is_err() {
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
    context: &ExecutionContext,
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
        if file.stamp != result.task.stamp {
            result.unstable = true;
            return Err(io::Error::other(
                "checkpoint source changed since discovery",
            ));
        }
        let hashes = hash_evidence(file, &options.hash, || context.checkpoint())?;
        result.unstable = !hashes.stable;
        result.hashes = Some(hashes);
        if result.unstable {
            return Err(io::Error::other("file changed during hashing"));
        }
        let batch = backend.extract(
            file,
            result.hashes.as_ref().expect("hash result exists"),
            options,
            context,
        )?;
        result.records = batch.records;
        result.cache_hit = batch.cache_hit;
        context.checkpoint()?;
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
    if !bound_records(&mut result.records, options.max_result_bytes) {
        result.error = Some("retained metadata result limit reached; records are partial".into());
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
