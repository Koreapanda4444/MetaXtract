use crate::{
    analysis,
    core::scan::*,
    schema::*,
    storage::{CaseWorkspace, ScanStatus},
};
use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct ScanRequest {
    pub source: PathBuf,
    pub case_path: PathBuf,
    pub journal_path: PathBuf,
    pub workers: usize,
    pub resume: Option<(u64, String)>,
    pub options: ScanOptions,
}
impl ScanRequest {
    pub fn new(source: PathBuf, case_path: PathBuf) -> Self {
        let mut journal = case_path.as_os_str().to_owned();
        journal.push(".scan.sqlite");
        Self {
            source,
            case_path,
            journal_path: journal.into(),
            workers: 2,
            resume: None,
            options: ScanOptions::default(),
        }
    }
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AppScanEvent {
    pub scan_id: String,
    pub run: u64,
    pub kind: String,
    pub path: Option<String>,
    pub summary: ScanSummary,
    pub error: Option<String>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ScanOutcome {
    pub scan_id: String,
    pub run: u64,
    pub summary: ScanSummary,
    #[serde(default)]
    pub errors: Vec<ScanFailure>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ScanFailure {
    pub path: String,
    pub message: String,
}
fn invalid(s: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, s)
}
pub fn analyze_record(record: &mut ExtractionRecord) -> io::Result<()> {
    let limits = analysis::AnalysisLimits::default();
    let result = analysis::normalize::normalize(record, limits)?;
    record.claims.extend(result.claims);
    let result = analysis::validation::reconcile(record, limits)?;
    record.claims = result.claims;
    for conflict in result.conflicts {
        if !record.conflicts.contains(&conflict) {
            record.conflicts.push(conflict);
        }
    }
    Ok(())
}
fn ingest(case: &CaseWorkspace, scan: &str, file: &ScannedFile) -> io::Result<()> {
    let key = format!("application.file.{}", file.task.id);
    if case.scan_receipt(scan, &key)?.is_some() {
        return Ok(());
    }
    let mut records = file.records.clone();
    for record in &mut records {
        if let Err(e) = analyze_record(record) {
            record.diagnostics.push(Diagnostic {
                code: "analysis.limit".into(),
                severity: DiagnosticSeverity::Warning,
                stage: DiagnosticStage::ResourceLimit,
                message: e.to_string(),
                parser: None,
                source: record.source.path.clone(),
                source_range: None,
                related_observations: vec![],
                recoverable: true,
            });
        }
    }
    case.append_records_with_receipt(scan,&records,&key,&serde_json::json!({"task":file.task,"hashes":file.hashes,"error":file.error,"unstable":file.unstable}))?;
    Ok(())
}
pub fn run(
    request: &ScanRequest,
    control: ScanControl,
    mut sink: impl FnMut(AppScanEvent) -> io::Result<()>,
) -> io::Result<ScanOutcome> {
    if request.workers == 0 || request.workers > 32 {
        return Err(invalid("workers must be 1..32"));
    }
    let case = if request.case_path.exists() {
        crate::open_case(&request.case_path, true)?
    } else {
        CaseWorkspace::create(&request.case_path, "MetaXtract case")?
    };
    let root = ScanRoot::open(&request.source, SymlinkPolicy::Skip)?;
    if request.journal_path.exists() {
        let candidate = ScanRoot::open(&request.journal_path, SymlinkPolicy::Skip)?;
        let selected = ScanRoot::open(case.path(), SymlinkPolicy::Skip)?;
        if candidate.stamp.device == selected.stamp.device
            && candidate.stamp.file_id == selected.stamp.file_id
        {
            return Err(invalid(
                "scan checkpoint must be separate from the case file",
            ));
        }
    }
    let store = ScanStore::open(&request.journal_path)?;
    let journal = NativePath::from_path(&request.journal_path.canonicalize()?);
    let backend: Arc<dyn ExtractionBackend> = BundledWorker::adjacent()?;
    let mut progress = ScanSummary::default();
    let (scan, resuming) = if let Some((run, id)) = &request.resume {
        let receipt = case
            .scan_receipt(id, "application.session")?
            .ok_or_else(|| invalid("scan has no matching resumable session"))?;
        if receipt.get("run").and_then(|v| v.as_u64()) != Some(*run)
            || receipt.get("journal")
                != Some(&serde_json::to_value(&journal).map_err(io::Error::other)?)
        {
            return Err(invalid(
                "checkpoint journal does not belong to the selected case scan",
            ));
        }
        let info = case
            .scan_info(id)?
            .ok_or_else(|| invalid("unknown case scan"))?;
        match info.status {
            ScanStatus::Running => {}
            ScanStatus::Paused => case.resume_scan(id)?,
            _ => return Err(invalid("only paused or interrupted scans can resume")),
        }
        let mut after = 0;
        loop {
            let page = store.page_tasks(*run, after, 128)?;
            if page.is_empty() {
                break;
            }
            for row in page {
                after = row.task.id;
                progress.discovered += 1;
                if row.state == TaskState::Done {
                    if let Some(file) = store.read_result(*run, after)? {
                        ingest(&case, id, &file)?;
                        progress.completed += 1;
                        progress.failed += u64::from(file.failed());
                        progress.unstable += u64::from(file.unstable);
                        progress.bytes_hashed += file.hashes.as_ref().map_or(0, |h| h.bytes_read);
                    }
                }
            }
        }
        (id.clone(), true)
    } else {
        (
            case.begin_scan(&request.source.to_string_lossy(), "rust.native")?,
            false,
        )
    };
    let mut options = request.options.clone();
    options.workers = request.workers;
    for path in [case.path(), request.journal_path.as_path()] {
        for suffix in ["", "-journal", "-wal", "-shm"] {
            let mut p = path.as_os_str().to_owned();
            p.push(suffix);
            if let Some(relative) = root.relative_to_root(Path::new(&p)) {
                options.walk.excluded.push(relative);
            }
        }
    }
    let mut saved = resuming;
    let mut failures = Vec::new();
    let mut forward = |run, event: ScanEvent| -> io::Result<()> {
        if !saved {
            case.append_records_with_receipt(
                &scan,
                &[],
                "application.session",
                &serde_json::json!({"run":run,"journal":journal,"root":root.original}),
            )?;
            saved = true;
        }
        let error = if let ScanEvent::Completed(file) = &event {
            file.error.clone().or_else(|| {
                file.records
                    .iter()
                    .filter(|r| r.status == ExtractionStatus::Failed)
                    .flat_map(|r| &r.diagnostics)
                    .next()
                    .map(|d| d.message.clone())
            })
        } else {
            None
        };
        if let (Some(error), ScanEvent::Completed(file)) = (&error, &event) {
            if failures.len() < 64 {
                failures.push(ScanFailure {
                    path: file.task.evidence_path.to_string(),
                    message: error.chars().take(4096).collect(),
                });
            }
        }
        let (kind, path) = match &event {
            ScanEvent::Discovered(t) => {
                progress.discovered += 1;
                ("discovered", Some(t.evidence_path.to_string()))
            }
            ScanEvent::Resumed(t) => ("resumed", Some(t.evidence_path.to_string())),
            ScanEvent::Started(t) => ("started", Some(t.evidence_path.to_string())),
            ScanEvent::Completed(file) => {
                ingest(&case, &scan, file)?;
                progress.completed += 1;
                progress.failed += u64::from(file.failed());
                progress.unstable += u64::from(file.unstable);
                progress.bytes_hashed += file.hashes.as_ref().map_or(0, |h| h.bytes_read);
                ("completed", Some(file.task.evidence_path.to_string()))
            }
            ScanEvent::Notice { message, .. } => {
                progress.notices += 1;
                ("notice", Some(message.clone()))
            }
            ScanEvent::DiscoveryFinished => ("discovery_finished", None),
            ScanEvent::Finished(summary) => {
                progress = summary.clone();
                ("finished", None)
            }
        };
        sink(AppScanEvent {
            scan_id: scan.clone(),
            run,
            kind: kind.into(),
            path,
            summary: progress.clone(),
            error,
        })
    };
    let result = if let Some((run, _)) = &request.resume {
        store
            .resume(*run, Arc::clone(&root), backend, control, &mut forward)
            .map(|s| (*run, s))
    } else {
        store.scan(Arc::clone(&root), options, backend, control, &mut forward)
    };
    match result {
        Ok((run, summary)) => {
            case.finish_scan(
                &scan,
                match summary.stopped {
                    None => ScanStatus::Completed,
                    Some(StopReason::Cancelled) => ScanStatus::Cancelled,
                    Some(_) => ScanStatus::Paused,
                },
            )?;
            Ok(ScanOutcome {
                scan_id: scan,
                run,
                summary,
                errors: failures,
            })
        }
        Err(e) => {
            let _ = case.finish_scan(&scan, ScanStatus::Paused);
            Err(e)
        }
    }
}
