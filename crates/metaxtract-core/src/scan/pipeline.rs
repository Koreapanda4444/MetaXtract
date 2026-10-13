use super::{
    EvidenceHashes, ExecutionContext, FileStamp, HashOptions, NativePath, OpenedEvidence,
    ScanControl, ScanRoot, StopReason, WalkEntry, WalkOptions,
    hash::hash_capture,
    memory::{MemoryBudget, MemoryLease, bound_records, record_bytes},
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
    #[serde(default = "default_window")]
    pub scheduling_window: usize,
    pub max_memory_bytes: u64,
    pub max_result_bytes: u64,
    pub file_wall_time_ms: u64,
    pub scan_wall_time_ms: Option<u64>,
}
fn default_window() -> usize {
    16
}
impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            walk: WalkOptions::default(),
            hash: HashOptions::default(),
            recursive: RecursiveLimits::default(),
            workers: thread::available_parallelism().map_or(2, |n| n.get().min(4)),
            queue_capacity: 8,
            scheduling_window: 16,
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
        if !(1..=32).contains(&self.workers)
            || !(1..=256).contains(&self.queue_capacity)
            || !(1..=256).contains(&self.scheduling_window)
        {
            return Err(invalid("invalid scan concurrency"));
        }
        if self.file_wall_time_ms == 0
            || self.file_wall_time_ms > 86_400_000
            || self
                .scan_wall_time_ms
                .is_some_and(|v| v == 0 || v > 31 * 86_400_000)
            || self.max_result_bytes < 1024 * 1024
            || self.max_result_bytes > 256 * 1024 * 1024
            || self
                .reservation()
                .saturating_add((self.workers as u64).saturating_mul(self.hash.buffer_bytes as u64))
                > self.max_memory_bytes
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
        let probe_limits = metaxtract_native::ParserLimits {
            max_read_bytes: 64 * 1024,
            max_allocation_bytes: 1024 * 1024,
            max_wall_time_ms: context.remaining_ms().min(u64::from(u32::MAX)) as u32,
            ..limits.parser
        };
        let probe = metaxtract_native::BoundedInput::from_file(
            evidence.file.try_clone()?,
            probe_limits.input_limits(),
        )
        .map_err(|e| io::Error::other(e.to_string()))?;
        let candidates = metaxtract_native::detect_content(&probe, probe_limits)
            .map_err(|e| io::Error::other(e.to_string()))?;
        if let Some(root) = records.first_mut() {
            if candidates.len() == 1
                && matches!(
                    candidates[0].state,
                    metaxtract_native::DetectionState::Signature
                        | metaxtract_native::DetectionState::HeaderValidated
                )
            {
                root.source.detected_media_type = Some(candidates[0].mime.clone());
            }
            let name = evidence.relative.to_path()?;
            let extension = name.extension().and_then(|e| e.to_str());
            let assessment = crate::assess_content(&candidates, extension, None, None);
            for conflict in assessment.conflicts {
                root.diagnostics.push(Diagnostic {
                    code: format!("content.{:?}", conflict.kind),
                    severity: DiagnosticSeverity::Information,
                    stage: DiagnosticStage::Probe,
                    message: conflict.explanation,
                    parser: None,
                    source: root.source.path.clone(),
                    source_range: None,
                    related_observations: Vec::new(),
                    recoverable: true,
                });
            }
        }
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
                let mut window = Vec::with_capacity(opts.scheduling_window);
                for entry in root.walk(opts.walk.clone())? {
                    producer_control.checkpoint()?;
                    match entry {
                        WalkEntry::File(mut file) => {
                            next += 1;
                            let cost = file_cost(&mut file);
                            let task = FileTask {
                                id: next,
                                relative: file.relative.clone(),
                                evidence_path: file.evidence_path.clone(),
                                stamp: file.stamp.clone(),
                            };
                            window.push((cost, task, file));
                            if window.len() >= opts.scheduling_window {
                                dispatch_window(
                                    &mut window,
                                    false,
                                    producer_journal.as_ref(),
                                    &jobs_tx,
                                    &tx,
                                    &producer_control,
                                )?;
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
                dispatch_window(
                    &mut window,
                    true,
                    producer_journal.as_ref(),
                    &jobs_tx,
                    &tx,
                    &producer_control,
                )?;
                producer_control.checkpoint()?;
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
                let Ok(_hash_buffer_lease) =
                    memory.acquire(opts.hash.buffer_bytes as u64, &worker_control)
                else {
                    return;
                };
                let mut hash_buffer = Vec::new();
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
                        Ok(mut file) => process(
                            task,
                            &mut file,
                            opts,
                            backend.as_ref(),
                            &context,
                            &mut hash_buffer,
                        ),
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
                            event: ScanEvent::Completed(Box::new(result)),
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
        let mut batch = Vec::new();
        let mut batch_bytes = 0_u64;
        loop {
            match events_rx.recv_timeout(Duration::from_millis(25)) {
                Ok(envelope @ Envelope::Completed { .. }) => {
                    if let Envelope::Completed {
                        event: ScanEvent::Completed(file),
                        ..
                    } = &envelope
                    {
                        batch_bytes = batch_bytes.saturating_add(
                            file.records
                                .iter()
                                .fold(0_u64, |n, r| n.saturating_add(record_bytes(r))),
                        );
                    }
                    batch.push(envelope);
                    if batch.len() >= 16 || batch_bytes >= 16 * 1024 * 1024 {
                        flush_batch(&mut batch, &journal, &mut summary, &mut sink, &control)?;
                        batch_bytes = 0;
                    }
                }
                Ok(Envelope::Event(event)) => {
                    flush_batch(&mut batch, &journal, &mut summary, &mut sink, &control)?;
                    batch_bytes = 0;
                    match &event {
                        ScanEvent::Discovered(_) => summary.discovered += 1,
                        ScanEvent::Notice { .. } => summary.notices += 1,
                        _ => {}
                    }
                    if let Err(error) = sink(event) {
                        control.cancel();
                        return Err(error);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    flush_batch(&mut batch, &journal, &mut summary, &mut sink, &control)?;
                    batch_bytes = 0;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    flush_batch(&mut batch, &journal, &mut summary, &mut sink, &control)?;
                    break;
                }
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
        event: ScanEvent,
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
    buffer: &mut Vec<u8>,
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
        let hashes = hash_capture(file, &options.hash, || context.checkpoint(), None, buffer)?;
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

fn file_cost(file: &mut OpenedEvidence) -> u64 {
    use std::io::{Read, Seek, SeekFrom};
    let mut prefix = [0_u8; 4096];
    let _ = file.file.seek(SeekFrom::Start(0));
    let n = file.file.read(&mut prefix).unwrap_or(0);
    let _ = file.file.seek(SeekFrom::Start(0));
    let name = file.relative.name_bytes().unwrap_or_default();
    let weight = crate::containers::selected_parsers(&prefix[..n], &name)
        .iter()
        .map(|id| match *id {
            "images.raw" => 16,
            "media.inspect" => 12,
            "container.archive" | "pdf.metadata" => 10,
            "document.ole" | "document.package" => 8,
            _ => 2,
        })
        .sum::<u64>()
        .max(1);
    file.stamp
        .size_bytes
        .saturating_div(4096)
        .saturating_add(1)
        .saturating_mul(weight)
}
fn dispatch_window(
    window: &mut Vec<(u64, FileTask, OpenedEvidence)>,
    heavy_first: bool,
    journal: &dyn super::checkpoint::ScanJournal,
    jobs: &SyncSender<(FileTask, io::Result<OpenedEvidence>)>,
    events: &SyncSender<Envelope>,
    control: &ScanControl,
) -> io::Result<()> {
    window.sort_by_key(|(cost, _, _)| if heavy_first { *cost } else { u64::MAX - *cost });
    while let Some((_, task, file)) = window.pop() {
        control.checkpoint()?;
        if let Some(task) = journal.schedule(task)? {
            if events
                .send(Envelope::Event(ScanEvent::Discovered(task.clone())))
                .is_err()
                || !send_job(jobs, (task, Ok(file)), control)
            {
                control.checkpoint()?;
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "dispatch interrupted",
                ));
            }
        }
    }
    Ok(())
}
fn flush_batch(
    batch: &mut Vec<Envelope>,
    journal: &Arc<dyn super::checkpoint::ScanJournal>,
    summary: &mut ScanSummary,
    sink: &mut impl FnMut(ScanEvent) -> io::Result<()>,
    control: &ScanControl,
) -> io::Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let events: Vec<_> = batch
        .iter()
        .filter_map(|e| match e {
            Envelope::Completed { event, .. } => Some(event),
            _ => None,
        })
        .collect();
    let refs = events;
    if let Err(error) = journal.persist_batch(&refs) {
        control.cancel();
        return Err(error);
    }
    for envelope in batch.drain(..) {
        if let Envelope::Completed {
            event: ScanEvent::Completed(file),
            _lease,
        } = envelope
        {
            summary.completed += 1;
            summary.failed += u64::from(file.failed());
            summary.unstable += u64::from(file.unstable);
            summary.bytes_hashed = summary
                .bytes_hashed
                .saturating_add(file.hashes.as_ref().map_or(0, |h| h.bytes_read));
            if let Err(error) = sink(ScanEvent::Completed(file)) {
                control.cancel();
                return Err(error);
            }
        }
    }
    Ok(())
}
