use super::{
    EvidenceHashes, ExecutionContext, ExtractionBackend, ExtractionBatch, FileStamp, FileTask,
    OpenedEvidence, ScanControl, ScanEvent, ScanOptions, ScanRoot, ScanSummary, codec,
    engine_fingerprint, hash::hash_capture, memory::bound_records, walk::invalid,
};
use crate::schema::{
    Diagnostic, DiagnosticSeverity, DiagnosticStage, EvidenceKind, ExtractionRecord,
    ExtractionStatus, SourceKind,
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};
use tempfile::{NamedTempFile, TempDir};
const MAGIC: &[u8; 8] = b"MXWR1\0\0\0";
const REQUEST_LIMIT: usize = 2 * 1024 * 1024;
#[derive(serde::Serialize, serde::Deserialize)]
struct Request {
    engine: String,
    task: FileTask,
    options: ScanOptions,
    snapshot: FileStamp,
    hashes: EvidenceHashes,
    remaining_ms: u64,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Response {
    engine: String,
    task_id: u64,
    records: Vec<ExtractionRecord>,
    error: Option<String>,
}
pub struct BundledWorker {
    path: PathBuf,
    snapshots: TempDir,
}
impl BundledWorker {
    pub fn adjacent() -> io::Result<Arc<Self>> {
        let exe = std::env::current_exe()?;
        Self::from_path(
            exe.parent()
                .ok_or_else(|| invalid("executable has no parent"))?
                .join(worker_name()),
        )
    }
    pub fn from_path(path: impl AsRef<Path>) -> io::Result<Arc<Self>> {
        let path = path.as_ref();
        if path.file_name() != Some(std::ffi::OsStr::new(worker_name())) {
            return Err(invalid(
                "worker must be the bundled metaxtract-worker executable",
            ));
        }
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.is_symlink() {
            return Err(invalid("worker is not a regular bundled executable"));
        }
        Ok(Arc::new(Self {
            path: path.canonicalize()?,
            snapshots: tempfile::Builder::new()
                .prefix(".metaxtract-worker-")
                .tempdir()?,
        }))
    }
}
fn worker_name() -> &'static str {
    if cfg!(windows) {
        "metaxtract-worker.exe"
    } else {
        "metaxtract-worker"
    }
}
impl ExtractionBackend for BundledWorker {
    fn fingerprint(&self) -> String {
        engine_fingerprint().to_owned()
    }
    fn excluded_paths(&self) -> Vec<PathBuf> {
        vec![self.snapshots.path().to_owned()]
    }
    fn extract(
        &self,
        evidence: &OpenedEvidence,
        hashes: &EvidenceHashes,
        options: &ScanOptions,
        context: &ExecutionContext,
    ) -> io::Result<ExtractionBatch> {
        context.checkpoint()?;
        let mut captured = NamedTempFile::new_in(self.snapshots.path())?;
        let mut opened = OpenedEvidence {
            file: evidence.file.try_clone()?,
            relative: evidence.relative.clone(),
            evidence_path: evidence.evidence_path.clone(),
            stamp: evidence.stamp.clone(),
        };
        let capture = hash_capture(
            &mut opened,
            &options.hash,
            || context.checkpoint(),
            Some(captured.as_file_mut()),
            &mut Vec::new(),
        )?;
        if !capture.stable
            || capture.digests != hashes.digests
            || capture.bytes_read != hashes.bytes_read
        {
            return Err(io::Error::other("source changed before worker capture"));
        }
        let input = File::open(captured.path())?;
        let snapshot = FileStamp::read(&input)?;
        let task = FileTask {
            id: 0,
            relative: evidence.relative.clone(),
            evidence_path: evidence.evidence_path.clone(),
            stamp: evidence.stamp.clone(),
        };
        let request = Request {
            engine: engine_fingerprint().into(),
            task,
            options: options.clone(),
            snapshot,
            hashes: hashes.clone(),
            remaining_ms: context.remaining_ms(),
        };
        let mut control = NamedTempFile::new_in(self.snapshots.path())?;
        control
            .as_file_mut()
            .write_all(&codec::encode(&request, REQUEST_LIMIT)?)?;
        control.as_file_mut().flush()?;
        let request_input = File::open(control.path())?;
        let mut command = Command::new(&self.path);
        command
            .arg("--protocol=1")
            .current_dir(self.snapshots.path())
            .stdin(Stdio::from(input))
            .stderr(Stdio::from(request_input))
            .stdout(Stdio::piped());
        library_environment(&mut command, &self.path)?;
        let mut child = RunningChild(command.spawn()?);
        let stdout = child
            .0
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("worker stdout is missing"))?;
        let max =
            (options.max_result_bytes.saturating_mul(4)).min(codec::MAX_BLOB_BYTES as u64) as usize;
        let (tx, rx) = mpsc::sync_channel(1);
        let reader = thread::spawn(move || {
            let _ = tx.send(read_frame(stdout, max));
        });
        let result = (|| -> io::Result<Response> {
            let mut response = None;
            loop {
                context.checkpoint()?;
                match rx.try_recv() {
                    Ok(value) => response = Some(value?),
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) if response.is_none() => {
                        return Err(io::Error::other("worker response was lost"));
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {}
                }
                if let Some(status) = child.0.try_wait()? {
                    if !status.success() {
                        return Err(io::Error::other(format!("native worker exited: {status}")));
                    }
                    if let Some(response) = response {
                        return Ok(response);
                    }
                }
                thread::sleep(Duration::from_millis(10));
            }
        })();
        drop(child);
        let _ = reader.join();
        let response = result?;
        if response.engine != engine_fingerprint() || response.task_id != 0 {
            return Err(invalid("worker protocol engine or task differs"));
        }
        if let Some(error) = response.error {
            return Err(io::Error::other(error));
        }
        validate_records(&response.records, evidence, options)?;
        context.checkpoint()?;
        Ok(ExtractionBatch {
            records: response.records,
            cache_hit: false,
        })
    }
}
struct RunningChild(Child);
impl Drop for RunningChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn library_environment(command: &mut Command, worker: &Path) -> io::Result<()> {
    let parent = worker
        .parent()
        .ok_or_else(|| invalid("worker has no parent"))?;
    for name in ["PATH", "LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH"] {
        let mut paths = vec![parent.join("lib"), parent.join("../lib"), parent.to_owned()];
        if let Some(existing) = std::env::var_os(name) {
            paths.extend(std::env::split_paths(&existing));
        }
        command.env(name, std::env::join_paths(paths).map_err(io::Error::other)?);
    }
    Ok(())
}
fn read_frame(mut input: impl Read, max: usize) -> io::Result<Response> {
    let mut header = [0_u8; 48];
    input.read_exact(&mut header)?;
    if &header[..8] != MAGIC {
        return Err(invalid("invalid bundled worker frame"));
    }
    let size = u64::from_le_bytes(header[8..16].try_into().expect("fixed header"));
    if size > max as u64 {
        return Err(invalid("worker response exceeds wire budget"));
    }
    let mut data = Vec::new();
    data.try_reserve_exact(size as usize)
        .map_err(|e| io::Error::other(e.to_string()))?;
    data.resize(size as usize, 0);
    input.read_exact(&mut data)?;
    let mut extra = [0_u8; 1];
    if input.read(&mut extra)? != 0 {
        return Err(invalid("worker response has trailing data"));
    }
    if Sha256::digest(&data).as_slice() != &header[16..48] {
        return Err(invalid("worker response checksum differs"));
    }
    codec::decode(&data, max)
}
fn write_frame(mut output: impl Write, response: &Response, max: usize) -> io::Result<()> {
    let data = codec::encode(response, max)?;
    output.write_all(MAGIC)?;
    output.write_all(&(data.len() as u64).to_le_bytes())?;
    output.write_all(&Sha256::digest(&data))?;
    output.write_all(&data)?;
    output.flush()
}
pub fn run_bundled_worker_stdio() -> io::Result<()> {
    let mut request_file = stdio_file(true)?;
    let mut request_bytes = Vec::new();
    Read::by_ref(&mut request_file)
        .take((REQUEST_LIMIT + 1) as u64)
        .read_to_end(&mut request_bytes)?;
    let request: Request = codec::decode(&request_bytes, REQUEST_LIMIT)?;
    let max = (request.options.max_result_bytes.saturating_mul(4)).min(codec::MAX_BLOB_BYTES as u64)
        as usize;
    let result = extract_request(&request);
    let response = match result {
        Ok(records) => Response {
            engine: engine_fingerprint().into(),
            task_id: request.task.id,
            records,
            error: None,
        },
        Err(error) => Response {
            engine: engine_fingerprint().into(),
            task_id: request.task.id,
            records: Vec::new(),
            error: Some(error.to_string()),
        },
    };
    write_frame(std::io::stdout().lock(), &response, max)
}
fn stdio_file(request: bool) -> io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::fd::AsFd;
        if request {
            Ok(File::from(std::io::stderr().as_fd().try_clone_to_owned()?))
        } else {
            Ok(File::from(std::io::stdin().as_fd().try_clone_to_owned()?))
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsHandle;
        if request {
            Ok(File::from(
                std::io::stderr().as_handle().try_clone_to_owned()?,
            ))
        } else {
            Ok(File::from(
                std::io::stdin().as_handle().try_clone_to_owned()?,
            ))
        }
    }
}
fn extract_request(request: &Request) -> io::Result<Vec<ExtractionRecord>> {
    if request.engine != engine_fingerprint() || request.remaining_ms == 0 {
        return Err(invalid("worker engine or deadline differs"));
    }
    request.options.validate()?;
    if !metaxtract_native::configure_worker_memory_limit(request.options.max_memory_bytes) {
        return Err(io::Error::other("could not apply worker memory limit"));
    }
    let file = stdio_file(false)?;
    let stamp = FileStamp::read(&file)?;
    if stamp != request.snapshot {
        return Err(invalid("worker input identity differs"));
    }
    let mut evidence = OpenedEvidence {
        file,
        relative: request.task.relative.clone(),
        evidence_path: request.task.evidence_path.clone(),
        stamp,
    };
    let control = ScanControl::new();
    let context = control.context(request.remaining_ms.min(request.options.file_wall_time_ms));
    let result =
        super::NativeExtractor.extract(&evidence, &request.hashes, &request.options, &context)?;
    let verified = super::hash_evidence(&mut evidence, &request.options.hash, || {
        context.checkpoint()
    })?;
    if !verified.stable
        || verified.digests != request.hashes.digests
        || verified.bytes_read != request.hashes.bytes_read
    {
        return Err(invalid("worker snapshot changed during extraction"));
    }
    let mut records = result.records;
    if !bound_records(&mut records, request.options.max_result_bytes) {
        if let Some(root) = records.first_mut() {
            root.status = ExtractionStatus::Partial;
            root.diagnostics.push(Diagnostic {
                code: "worker.result_budget".into(),
                severity: DiagnosticSeverity::Warning,
                stage: DiagnosticStage::ResourceLimit,
                message: "Retained metadata limit reached; the records are partial".into(),
                parser: None,
                source: root.source.path.clone(),
                source_range: None,
                related_observations: Vec::new(),
                recoverable: true,
            });
        }
    }
    if let Some(root) = records.first_mut() {
        root.source.identity = request.task.stamp.identity();
    }
    context.checkpoint()?;
    Ok(records)
}
fn validate_records(
    records: &[ExtractionRecord],
    evidence: &OpenedEvidence,
    options: &ScanOptions,
) -> io::Result<()> {
    if records.is_empty() || records.len() > options.recursive.max_nodes {
        return Err(invalid("invalid worker record count"));
    }
    let root = &records[0];
    if root.source.kind != SourceKind::File
        || root.source.path.depth() != 0
        || root.source.path.root_path() != &evidence.evidence_path
        || root.source.identity.size_bytes != evidence.stamp.size_bytes
    {
        return Err(invalid("worker root evidence differs"));
    }
    let mut paths = HashSet::new();
    let mut observations = 0_u64;
    let mut global_ids = HashSet::new();
    for record in records {
        if record.source.path.root_path() != &evidence.evidence_path
            || record.source.path.depth() > options.recursive.max_depth as usize
            || !paths.insert(record.source.path.clone())
        {
            return Err(invalid("worker evidence graph differs"));
        }
        let kind = match record.source.path.nested().last().map(|s| s.kind) {
            None => SourceKind::File,
            Some(EvidenceKind::ArchiveMember) => SourceKind::ArchiveMember,
            Some(EvidenceKind::Attachment) => SourceKind::Attachment,
            Some(EvidenceKind::EmbeddedObject) => SourceKind::EmbeddedObject,
            Some(EvidenceKind::AlternateStream | EvidenceKind::Resource) => SourceKind::Stream,
        };
        if record.source.kind != kind {
            return Err(invalid("worker evidence kind differs"));
        }
        let mut ids = HashSet::new();
        for observation in &record.observations {
            observations += 1;
            if observations > u64::from(options.recursive.parser.max_observations)
                || !ids.insert(observation.id)
                || !global_ids.insert(observation.id)
                || observation.provenance.source != record.source.path
            {
                return Err(invalid("worker observation identity differs"));
            }
            if observation
                .provenance
                .source_range
                .is_some_and(|r| r.end() > record.source.identity.size_bytes)
            {
                return Err(invalid("worker observation range exceeds source"));
            }
        }
        for claim in &record.claims {
            if claim
                .supporting_observations
                .iter()
                .chain(&claim.contradicting_observations)
                .any(|id| !ids.contains(id))
            {
                return Err(invalid("worker claim references missing observations"));
            }
        }
        for diagnostic in &record.diagnostics {
            if diagnostic.source != record.source.path
                || diagnostic
                    .source_range
                    .is_some_and(|r| r.end() > record.source.identity.size_bytes)
                || diagnostic
                    .related_observations
                    .iter()
                    .any(|id| !ids.contains(id))
            {
                return Err(invalid("worker diagnostic evidence differs"));
            }
        }
    }
    Ok(())
}
pub fn scan_local(
    root: Arc<ScanRoot>,
    options: ScanOptions,
    control: ScanControl,
    sink: impl FnMut(ScanEvent) -> io::Result<()>,
) -> io::Result<ScanSummary> {
    super::scan_stream_with_control(root, options, BundledWorker::adjacent()?, control, sink)
}
