//! Shared, bounded application operations for the CLI and desktop.
use crate::{analysis, core, schema::*, storage};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
};

pub fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    serde_json::from_slice(&crate::read_bytes(path, 16 * 1024 * 1024)?).map_err(io::Error::other)
}
pub fn read_text(path: &Path, max: usize) -> io::Result<String> {
    String::from_utf8(crate::read_bytes(path, max)?).map_err(io::Error::other)
}
/// Publish a complete output without replacing an existing destination.
pub fn write_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut out = tempfile::NamedTempFile::new_in(parent)?;
    out.write_all(bytes)?;
    out.as_file().sync_all()?;
    out.persist_noclobber(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}
pub fn write_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    write_bytes(
        path,
        &serde_json::to_vec_pretty(value).map_err(io::Error::other)?,
    )
}
pub fn profile(
    path: Option<&Path>,
    privacy: bool,
) -> io::Result<Option<analysis::redaction::RedactionProfile>> {
    let profile = if let Some(path) = path {
        Some(read_json::<analysis::redaction::RedactionProfile>(path)?)
    } else if privacy {
        Some(analysis::redaction::RedactionProfile::privacy())
    } else {
        None
    };
    if let Some(p) = &profile {
        p.validate()?;
    }
    Ok(profile)
}
pub fn report_options(
    title: &str,
    analyst: &str,
) -> io::Result<analysis::reporting::ReportOptions> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?;
    Ok(analysis::reporting::ReportOptions::new(
        title,
        analyst,
        i64::try_from(now.as_secs()).map_err(io::Error::other)?,
    ))
}
pub fn identity(path: &Path, name: &str, password: &str) -> io::Result<storage::SigningIdentity> {
    storage::SigningIdentity::load_encrypted(name, path, password, &Default::default())
}
pub fn cache_path(case: &Path) -> PathBuf {
    let mut path = case.as_os_str().to_owned();
    path.push(".cache.sqlite");
    path.into()
}
pub fn diagnostics(
    case: Option<&storage::CaseWorkspace>,
    scan: Option<&str>,
) -> io::Result<serde_json::Value> {
    let parsers = core::parser_capabilities().map_err(io::Error::other)?;
    let session = case
        .zip(scan)
        .map(|(c, id)| c.scan_receipt(id, "application.session"))
        .transpose()?
        .flatten();
    let worker = core::scan::BundledWorker::adjacent();
    Ok(
        serde_json::json!({"engine_fingerprint":core::scan::engine_fingerprint(),
        "worker_available":worker.is_ok(),"bundled_worker":worker.map(|_|"available".into()).unwrap_or_else(|e|e.to_string()),
        "stored_session":session,"parsers":parsers.into_iter().map(|p|serde_json::json!({
            "id":p.id,"library":p.name,"version":p.version,"formats":p.formats,"operations":p.operations})).collect::<Vec<_>>()}),
    )
}
#[derive(Serialize)]
pub struct EvidenceIssue {
    pub path: EvidencePath,
    pub message: String,
}
#[derive(Serialize)]
pub struct OriginalVerification {
    pub verified: bool,
    pub case_id: String,
    pub checked_files: u64,
    pub verified_files: u64,
    pub virtual_records_not_reopened: u64,
    pub issues: Vec<EvidenceIssue>,
}
/// Verify only caller-authorized root files; virtual records are reported separately.
pub fn verify_originals(
    case: &storage::CaseWorkspace,
    query: &storage::SearchQuery,
    source: &Path,
) -> io::Result<OriginalVerification> {
    use core::scan::{HashOptions, ScanRoot, SymlinkPolicy, hash_evidence};
    let info = case.info()?;
    case.verify_integrity(None)?;
    let root = ScanRoot::open(source, SymlinkPolicy::Skip)?;
    let limits = analysis::AnalysisLimits::default();
    let mut report = OriginalVerification {
        verified: false,
        case_id: info.case_id,
        checked_files: 0,
        verified_files: 0,
        virtual_records_not_reopened: 0,
        issues: vec![],
    };
    let mut cursor = None;
    let mut bytes = 0usize;
    loop {
        let page = case.search_records(query, cursor.as_ref(), 128)?;
        for header in page.items {
            if report.checked_files + report.virtual_records_not_reopened
                >= limits.max_records as u64
            {
                return Err(io::Error::other(
                    "original verification record budget exceeded",
                ));
            }
            let record = case
                .record(header.id)?
                .ok_or_else(|| io::Error::other("record disappeared"))?;
            if record.source.kind != SourceKind::File || !record.source.path.nested().is_empty() {
                report.virtual_records_not_reopened += 1;
                continue;
            }
            report.checked_files += 1;
            let result = (|| -> io::Result<()> {
                let expected = record
                    .digests
                    .iter()
                    .find(|d| d.algorithm == DigestAlgorithm::Sha256 && d.bytes.len() == 32)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "recorded SHA-256 is missing")
                    })?;
                let mut opened = root.open_evidence(record.source.path.root_path())?;
                if opened.stamp.size_bytes != record.source.identity.size_bytes {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "original size differs",
                    ));
                }
                let started = std::time::Instant::now();
                let hashes = hash_evidence(&mut opened, &HashOptions::default(), || {
                    if started.elapsed() > std::time::Duration::from_secs(120) {
                        Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "original verification deadline exceeded",
                        ))
                    } else {
                        Ok(())
                    }
                })?;
                if !hashes.stable
                    || hashes.bytes_read != record.source.identity.size_bytes
                    || !hashes.digests.contains(expected)
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "original SHA-256 differs or the file changed during verification",
                    ));
                }
                Ok(())
            })();
            if let Err(error) = result {
                let message = error.to_string();
                bytes = bytes
                    .saturating_add(record.source.path.to_string().len())
                    .saturating_add(message.len());
                if report.issues.len() >= limits.max_results || bytes > limits.max_bytes {
                    return Err(io::Error::other(
                        "original verification issue budget exceeded",
                    ));
                }
                report.issues.push(EvidenceIssue {
                    path: record.source.path,
                    message,
                });
            } else {
                report.verified_files += 1;
            }
        }
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    if case.info()?.revision != info.revision {
        return Err(io::Error::other(
            "case changed during original verification",
        ));
    }
    report.verified = report.issues.is_empty()
        && (report.checked_files > 0 || report.virtual_records_not_reopened == 0);
    Ok(report)
}
pub fn export_evidence(
    case: &storage::CaseWorkspace,
    query: &storage::SearchQuery,
    source: &Path,
    destination: &Path,
) -> io::Result<analysis::export::ExportReceipt> {
    let root = core::scan::ScanRoot::open(source, core::scan::SymlinkPolicy::Skip)?;
    analysis::export::export_original_evidence(
        case,
        query,
        destination,
        Default::default(),
        |record| {
            if record.source.kind != SourceKind::File || !record.source.path.nested().is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "select root files for original evidence export; virtual members require separately verified copies",
                ));
            }
            Ok(Box::new(
                root.open_evidence(record.source.path.root_path())?.file,
            ))
        },
    )
}
pub fn selected_query(
    scan: &str,
    ids: &std::collections::BTreeSet<i64>,
) -> io::Result<storage::SearchQuery> {
    if ids.is_empty() || ids.len() > 64 || ids.iter().any(|id| *id <= 0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "select 1..64 records",
        ));
    }
    let query = storage::SearchQuery::new(
        scan,
        storage::SearchExpression::Any(
            ids.iter()
                .map(|id| {
                    storage::SearchExpression::Predicate(storage::SearchPredicate {
                        field: storage::SearchField::RecordId,
                        operator: storage::SearchOperator::Equal,
                        value: id.to_string(),
                    })
                })
                .collect(),
        ),
    );
    query.validate()?;
    Ok(query)
}
