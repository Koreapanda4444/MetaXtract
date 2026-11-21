//! Rust owns output publication; the isolated C++ parser returns only bounded memory bytes.
use crate::{core::scan::*, schema::*};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
    path::PathBuf,
    sync::Arc,
};
pub struct SanitizeRequest {
    pub input: PathBuf,
    pub output: PathBuf,
    pub allow_signature_invalidation: bool,
    pub options: ScanOptions,
}
impl SanitizeRequest {
    pub fn new(input: PathBuf, output: PathBuf) -> Self {
        Self {
            input,
            output,
            allow_signature_invalidation: false,
            options: ScanOptions::default(),
        }
    }
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SanitizedCopy {
    pub output: NativePath,
    pub output_display: String,
    pub format: String,
    pub original_sha256: String,
    pub output_sha256: String,
    pub original_bytes: u64,
    pub output_bytes: u64,
    pub original_unchanged: bool,
    pub rendering_sha256: String,
    pub removed_metadata_blocks: u64,
    pub removed_trailing_bytes: u64,
    pub warnings: Vec<String>,
    pub policy: String,
    pub verification: CopyVerification,
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn observed<'a>(record: &'a ExtractionRecord, key: &str) -> Option<&'a RawValue> {
    record
        .observations
        .iter()
        .find(|o| {
            ((o.provenance.parser.id == "images.sanitize" && o.tag.namespace == "sanitize.image")
                || (o.provenance.parser.id == "documents.sanitize"
                    && o.tag.namespace == "sanitize.document"))
                && o.tag.name.as_deref() == Some(key)
        })
        .map(|o| o.interpreted_value.as_ref().unwrap_or(&o.raw_value))
}
fn text(record: &ExtractionRecord, key: &str) -> io::Result<String> {
    if let Some(RawValue::Text(value)) = observed(record, key) {
        if value.lossless {
            return Ok(value.text.clone());
        }
    }
    Err(invalid(
        "sanitized copy response lacks required text evidence",
    ))
}
fn number(record: &ExtractionRecord, key: &str) -> io::Result<u64> {
    match observed(record, key) {
        Some(RawValue::Unsigned(n)) => Ok(*n),
        Some(RawValue::Signed(n)) => {
            u64::try_from(*n).map_err(|_| invalid("invalid sanitized copy count"))
        }
        _ => Err(invalid(
            "sanitized copy response lacks required count evidence",
        )),
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn create_copy(request: &SanitizeRequest, control: ScanControl) -> io::Result<SanitizedCopy> {
    match std::fs::symlink_metadata(&request.output) {
        Ok(_) => return Err(invalid("sanitized copy destination already exists")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let root = ScanRoot::open(&request.input, SymlinkPolicy::Skip)?;
    if !root.is_single_file() {
        return Err(invalid("sanitized copies require one regular source file"));
    }
    let mut options = request.options.clone();
    options.sanitize_copy = true;
    options.workers = 1;
    options.walk.max_files = 1;
    options.walk.include_hidden = true;
    options.hash.sha256 = true;
    options.recursive.max_nodes = 1;
    if root.stamp.size_bytes
        > options
            .recursive
            .parser
            .max_member_bytes
            .min(options.recursive.parser.max_source_bytes)
    {
        return Err(invalid(
            "sanitized copy input exceeds the configured source/member budget",
        ));
    }
    let mut completed = None;
    let summary = scan_local(root.clone(), options.clone(), control.clone(), |event| {
        if let ScanEvent::Completed(file) = event {
            if completed.replace(file).is_some() {
                return Err(invalid("sanitized copy returned multiple input files"));
            }
        }
        Ok(())
    })?;
    if summary.stopped.is_some()
        || summary.completed != 1
        || summary.failed != 0
        || summary.unstable != 0
    {
        return Err(invalid(
            "sanitized copy scan did not finish with a stable source",
        ));
    }
    let file = completed.ok_or_else(|| invalid("sanitized copy source was not processed"))?;
    if file.unstable || file.error.is_some() || file.records.len() != 1 {
        return Err(invalid("sanitized copy source extraction failed"));
    }
    let record = &file.records[0];
    if record.status != ExtractionStatus::Complete {
        return Err(io::Error::other(
            record
                .diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("; "),
        ));
    }
    let before = text(record, "rendering_before_sha256")?;
    let after = text(record, "rendering_after_sha256")?;
    if before != after || before.len() != 64 || !before.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid(
            "sanitized copy rendering digest verification failed",
        ));
    }
    let authenticity = number(record, "authenticity_metadata_removed")? != 0;
    if authenticity && !request.allow_signature_invalidation {
        return Err(invalid(
            "embedded signatures would be invalidated; allow signature invalidation explicitly",
        ));
    }
    let Some(RawValue::Bytes(bytes)) = observed(record, "sanitized_payload") else {
        return Err(invalid(
            "sanitized copy payload was not retained within the result budget",
        ));
    };
    if bytes.is_empty() || bytes.len() as u64 > options.recursive.parser.max_member_bytes {
        return Err(invalid("sanitized copy output exceeds the member budget"));
    }
    let hashes = file
        .hashes
        .as_ref()
        .filter(|h| h.stable)
        .ok_or_else(|| invalid("original source digest is unavailable"))?;
    let original = hashes
        .digests
        .iter()
        .find(|h| h.algorithm == DigestAlgorithm::Sha256)
        .ok_or_else(|| invalid("original SHA-256 is unavailable"))?;
    let parent = request.output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| std::path::Path::new("."));
    let suffix = request.input.extension().and_then(|s| s.to_str()).map(|s| format!(".{s}")).unwrap_or_default();
    let mut candidate = tempfile::Builder::new()
        .prefix(".metaxtract-sanitize-")
        .suffix(&suffix)
        .tempfile_in(parent)?;
    candidate.write_all(bytes)?;
    candidate.as_file().sync_all()?;
    let mut inspect_options = options.clone();
    inspect_options.sanitize_copy = false;
    inspect_options.recursive.max_nodes = request.options.recursive.max_nodes;
    let source_record = inspect_copy(root.clone(), inspect_options.clone(), control.clone())?;
    if source_record.hashes.as_ref().is_none_or(|h| !h.stable || h.digests != hashes.digests) {
        return Err(invalid("original changed during before/after metadata verification"));
    }
    let candidate_root = ScanRoot::open(candidate.path(), SymlinkPolicy::Skip)?;
    let candidate_file = inspect_copy(candidate_root, inspect_options, control.clone())?;
    let candidate_hash = candidate_file.hashes.as_ref().filter(|h| h.stable)
        .and_then(|h| h.digests.iter().find(|d| d.algorithm == DigestAlgorithm::Sha256))
        .ok_or_else(|| invalid("sanitized output verification lacks SHA-256"))?;
    if candidate_hash.bytes.as_slice() != &Sha256::digest(bytes)[..]
        || candidate_file.records[0].source.detected_media_type != source_record.records[0].source.detected_media_type
    {
        return Err(invalid("sanitized output bytes or detected format changed during verification"));
    }
    let verification = compare_metadata(&source_record.records[0], &candidate_file.records[0])?;
    let mut reopened = root.open_file(&file.task.relative, SymlinkPolicy::Skip)?;
    if reopened.stamp != hashes.before {
        return Err(invalid(
            "original source identity changed before output publication",
        ));
    }
    let verified = hash_evidence(&mut reopened, &options.hash, || control.checkpoint())?;
    if !verified.stable || verified.digests != hashes.digests {
        return Err(invalid(
            "original source bytes changed before output publication",
        ));
    }
    control.checkpoint()?;
    let mut warnings=vec!["Color profiles, rendering metadata and visible content remain; this is not visual-content redaction or a guarantee of anonymity.".into()];
    if authenticity {
        warnings.push("Embedded signatures are invalidated by this copy. Retained document signature fields must not be treated as valid.".into());
    }
    if matches!(text(record, "format")?.as_str(), "pdf" | "ooxml" | "odf") {
        warnings.push("Only selected document properties are removed. Body text, comments, revisions, attachments, macros and nested metadata remain.".into());
    }
    let receipt = SanitizedCopy {
        output: NativePath::from_path(&request.output),
        output_display: request.output.to_string_lossy().into_owned(),
        format: text(record, "format")?,
        original_sha256: hex(&original.bytes),
        output_sha256: hex(&Sha256::digest(bytes)),
        original_bytes: hashes.bytes_read,
        output_bytes: bytes.len() as u64,
        original_unchanged: true,
        rendering_sha256: before,
        removed_metadata_blocks: number(record, "removed_metadata_blocks")?,
        removed_trailing_bytes: number(record, "removed_trailing_bytes")?,
        warnings,
        policy: text(record, "policy")?,
        verification,
    };
    let candidate_stamp = FileStamp::read(candidate.as_file())?;
    let verified_stamp = candidate_file.hashes.as_ref().ok_or_else(|| invalid("missing candidate identity"))?.after.clone();
    if candidate_stamp != verified_stamp {
        return Err(invalid("sanitized output identity changed before publication"));
    }
    control.checkpoint()?;
    candidate.persist_noclobber(&request.output).map_err(|error| error.error)?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(receipt)
}

pub fn property_mask(selection: &str) -> io::Result<u32> {
    let mut mask = 0;
    for name in selection.split(',') {
        let bit = match name {
            "author" => 1,
            "title" => 2,
            "description" => 4,
            "dates" => 8,
            "custom" => 16,
            "application" => 32,
            _ => return Err(invalid("unknown document property group")),
        };
        if mask & bit != 0 {
            return Err(invalid("duplicate document property group"));
        }
        mask |= bit;
    }
    Ok(mask)
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CopyVerification {
    pub before_observations: usize,
    pub after_observations: usize,
    pub removed_or_changed_fields: Vec<String>,
    pub retained_fields: Vec<String>,
    pub new_or_changed_fields: Vec<String>,
    pub field_lists_truncated: bool,
    pub scope: String,
}

fn inspect_copy(
    root: Arc<ScanRoot>,
    options: ScanOptions,
    control: ScanControl,
) -> io::Result<Box<ScannedFile>> {
    let mut completed = None;
    let summary = scan_local(root, options, control, |event| {
        if let ScanEvent::Completed(file) = event {
            if completed.replace(file).is_some() {
                return Err(invalid("copy verification returned multiple files"));
            }
        }
        Ok(())
    })?;
    let file = completed.ok_or_else(|| invalid("copy verification produced no result"))?;
    if summary.stopped.is_some() || summary.failed != 0 || summary.unstable != 0
        || file.unstable || file.error.is_some() || file.records.is_empty()
        || file.records[0].status != ExtractionStatus::Complete
    {
        return Err(invalid("copy verification requires a complete stable root extraction"));
    }
    Ok(file)
}

fn metadata_facts(record: &ExtractionRecord) -> io::Result<BTreeMap<String, BTreeSet<String>>> {
    let mut facts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for observation in &record.observations {
        let key = format!("{} / {} / {}", observation.provenance.parser.id,
            observation.tag.namespace, observation.tag.name.as_deref().unwrap_or("(numeric tag)"));
        let bytes = serde_json::to_vec(observation.interpreted_value.as_ref().unwrap_or(&observation.raw_value))
            .map_err(io::Error::other)?;
        facts.entry(key).or_default().insert(hex(&Sha256::digest(bytes)));
    }
    Ok(facts)
}

fn compare_metadata(before: &ExtractionRecord, after: &ExtractionRecord) -> io::Result<CopyVerification> {
    let old = metadata_facts(before)?;
    let new = metadata_facts(after)?;
    let removed: Vec<_> = old.iter().filter(|(key, values)| new.get(*key) != Some(*values)).map(|(key, _)| key.clone()).collect();
    let retained: Vec<_> = new.keys().cloned().collect();
    let added: Vec<_> = new.iter().filter(|(key, values)| old.get(*key) != Some(*values)).map(|(key, _)| key.clone()).collect();
    let truncated = removed.len() > 256 || retained.len() > 256 || added.len() > 256;
    Ok(CopyVerification {
        before_observations: before.observations.len(),
        after_observations: after.observations.len(),
        removed_or_changed_fields: removed.into_iter().take(256).collect(),
        retained_fields: retained.into_iter().take(256).collect(),
        new_or_changed_fields: added.into_iter().take(256).collect(),
        field_lists_truncated: truncated,
        scope: "Independent normal-parser re-extraction before publication; lists describe observed facts only. Retained properties, profiles, body content and unrecognized metadata may disclose information.".into(),
    })
}
