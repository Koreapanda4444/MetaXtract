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
    pub cache_enabled: bool,
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
            cache_enabled: true,
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
    #[serde(default)]
    pub engine_fingerprint: String,
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
    if record
        .observations
        .iter()
        .any(|o| o.tag.namespace.starts_with("image."))
    {
        let image = crate::core::reconcile_image_metadata_with_limits(
            &record.observations,
            crate::core::ImageAnalysisLimits {
                max_observations: limits.max_fields,
                max_alternatives_per_key: 64,
                max_claim_links: limits.max_comparisons,
            },
        )
        .map_err(|e| io::Error::other(format!("image reconciliation: {e:?}")))?;
        let signature = |c: &NormalizedClaim| {
            serde_json::to_vec(&(&c.key, &c.value, &c.supporting_observations))
                .map_err(io::Error::other)
        };
        let mut existing = std::collections::BTreeMap::new();
        for claim in &record.claims {
            existing.insert(signature(claim)?, claim.id);
        }
        let mut next = record
            .claims
            .iter()
            .map(|c| c.id.get())
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| io::Error::other("claim ID exhausted"))?;
        let mut ids = std::collections::BTreeMap::new();
        for mut claim in image.claims {
            let key = signature(&claim)?;
            if let Some(id) = existing.get(&key) {
                ids.insert(claim.id, *id);
                continue;
            }
            let id = ClaimId::new(next);
            next = next
                .checked_add(1)
                .ok_or_else(|| io::Error::other("claim ID exhausted"))?;
            ids.insert(claim.id, id);
            claim.id = id;
            existing.insert(key, id);
            record.claims.push(claim);
        }
        for mut conflict in image.conflicts {
            conflict.alternatives = conflict
                .alternatives
                .iter()
                .map(|id| {
                    ids.get(id)
                        .copied()
                        .ok_or_else(|| io::Error::other("missing image conflict claim"))
                })
                .collect::<io::Result<_>>()?;
            if !record.conflicts.contains(&conflict) {
                record.conflicts.push(conflict);
            }
        }
        for issue in image.issues {
            let diagnostic = Diagnostic {
                code: format!("analysis.image.{:?}", issue.kind),
                severity: DiagnosticSeverity::Information,
                stage: DiagnosticStage::Parse,
                message: issue.detail,
                parser: None,
                source: record.source.path.clone(),
                source_range: None,
                related_observations: issue.observations,
                recoverable: true,
            };
            if !record.diagnostics.contains(&diagnostic) {
                record.diagnostics.push(diagnostic);
            }
        }
    }
    let result = analysis::normalize::normalize(record, limits)?;
    record.claims.extend(result.claims);
    for issue in result.issues {
        let diagnostic = Diagnostic {
            code: "analysis.normalization".into(),
            severity: DiagnosticSeverity::Warning,
            stage: DiagnosticStage::Parse,
            message: issue.reason,
            parser: None,
            source: record.source.path.clone(),
            source_range: None,
            related_observations: vec![issue.observation],
            recoverable: true,
        };
        if !record.diagnostics.contains(&diagnostic) {
            record.diagnostics.push(diagnostic);
        }
    }
    package_media_type(record);
    let result = analysis::validation::reconcile(record, limits)?;
    record.claims = result.claims;
    for conflict in result.conflicts {
        if !record.conflicts.contains(&conflict) {
            record.conflicts.push(conflict);
        }
    }
    Ok(())
}
fn package_media_type(record: &mut ExtractionRecord) {
    if record.source.detected_media_type.as_deref() != Some("application/zip") {
        return;
    }
    fn text(o: &MetadataObservation) -> Option<&str> {
        match o.interpreted_value.as_ref().unwrap_or(&o.raw_value) {
            RawValue::Text(t) if t.lossless => Some(t.text.as_str()),
            _ => None,
        }
    }
    let mut validated = std::collections::BTreeSet::new();
    let mut part_names =
        std::collections::BTreeMap::<(&str, Option<&str>), std::collections::BTreeSet<&str>>::new();
    for o in &record.observations {
        if o.provenance.parser.id != "document.package" {
            continue;
        }
        if o.tag.name.as_deref() == Some("validated_main_part") {
            if let Some(value) = text(o) {
                validated.insert(value);
            }
        } else if o.tag.name.as_deref() == Some("PartName") {
            if let Some(value) = text(o) {
                part_names
                    .entry((o.tag.namespace.as_str(), o.provenance.stream_id.as_deref()))
                    .or_default()
                    .insert(value);
            }
        }
    }
    let mut candidates = std::collections::BTreeSet::new();
    for o in &record.observations {
        if o.provenance.parser.id != "document.package"
            || o.tag.name.as_deref() != Some("ContentType")
        {
            continue;
        }
        let (part, mime) = match text(o) {
            Some(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
            ) => (
                "word/document.xml",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml") => (
                "xl/workbook.xml",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ),
            Some(
                "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml",
            ) => (
                "ppt/presentation.xml",
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            ),
            _ => continue,
        };
        if validated.contains(part)
            && part_names
                .get(&(o.tag.namespace.as_str(), o.provenance.stream_id.as_deref()))
                .is_some_and(|names| {
                    names.len() == 1 && names.contains(format!("/{part}").as_str())
                })
        {
            candidates.insert(mime);
        }
    }
    if candidates.len() == 1 {
        record.source.detected_media_type =
            Some((*candidates.first().expect("one candidate")).into());
        record
            .diagnostics
            .retain(|d| d.code != "content.SubtypeUnverified");
    }
}

fn ingest(case: &CaseWorkspace, scan: &str, file: &mut ScannedFile) -> io::Result<()> {
    let key = format!("application.file.{}", file.task.id);
    if case.scan_receipt(scan, &key)?.is_some() {
        return Ok(());
    }
    let mut records = std::mem::take(&mut file.records);
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
    let mut backend: Arc<dyn ExtractionBackend> = BundledWorker::adjacent()?;
    if request.cache_enabled {
        let cache_path = crate::workflows::cache_path(case.path());
        if cache_path.exists() {
            let cache = ScanRoot::open(&cache_path, SymlinkPolicy::Skip)?;
            for path in [
                case.path(),
                request.journal_path.as_path(),
                request.source.as_path(),
            ] {
                let target = ScanRoot::open(path, SymlinkPolicy::Skip)?;
                if cache.stamp.device == target.stamp.device
                    && cache.stamp.file_id == target.stamp.file_id
                {
                    return Err(invalid(
                        "cache must differ from the case, journal and evidence root",
                    ));
                }
            }
        }
        backend = CachedExtractor::new(
            backend,
            RecordCache::open(cache_path, 256 * 1024 * 1024, 10000)?,
        );
    }
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
                    if let Some(mut file) = store.read_result(*run, after)? {
                        let failed = file.failed();
                        ingest(&case, id, &mut file)?;
                        progress.completed += 1;
                        progress.failed += u64::from(failed);
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
    let mut forward = |run, mut event: ScanEvent| -> io::Result<()> {
        if !saved {
            case.append_records_with_receipt(
                &scan,
                &[],
                "application.session",
                &serde_json::json!({"run":run,"journal":journal,"root":root.original,"engine_fingerprint":engine_fingerprint()}),
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
        let (kind, path) = match &mut event {
            ScanEvent::Discovered(t) => {
                progress.discovered += 1;
                ("discovered", Some(t.evidence_path.to_string()))
            }
            ScanEvent::Resumed(t) => ("resumed", Some(t.evidence_path.to_string())),
            ScanEvent::Started(t) => ("started", Some(t.evidence_path.to_string())),
            ScanEvent::Completed(file) => {
                let failed = file.failed();
                ingest(&case, &scan, file)?;
                progress.completed += 1;
                progress.failed += u64::from(failed);
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
            let status = match summary.stopped {
                None => ScanStatus::Completed,
                Some(StopReason::Cancelled) => ScanStatus::Cancelled,
                Some(_) => ScanStatus::Paused,
            };
            let outcome = ScanOutcome {
                scan_id: scan,
                run,
                summary,
                errors: failures,
                engine_fingerprint: engine_fingerprint().into(),
            };
            case.finish_scan_with_summary(&outcome.scan_id, status, &outcome)?;
            Ok(outcome)
        }
        Err(e) => {
            let _ = case.finish_scan(&scan, ScanStatus::Paused);
            Err(e)
        }
    }
}
