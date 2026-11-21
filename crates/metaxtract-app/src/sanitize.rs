//! Rust owns output publication; the isolated C++ parser returns only bounded memory bytes.
use crate::{core::scan::*, schema::*};
use sha2::{Digest, Sha256};
use std::{io, path::PathBuf};
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
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn observed<'a>(record: &'a ExtractionRecord, key: &str) -> Option<&'a RawValue> {
    record
        .observations
        .iter()
        .find(|o| {
            o.provenance.parser.id == "images.sanitize"
                && o.tag.namespace == "sanitize.image"
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
            "embedded authenticity metadata would be removed; allow signature invalidation explicitly",
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
        warnings.push("Embedded authenticity metadata was removed; its signatures no longer accompany this copy.".into());
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
    };
    crate::workflows::write_bytes(&request.output, bytes)?;
    Ok(receipt)
}
