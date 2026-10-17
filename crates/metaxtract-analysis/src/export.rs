use crate::{
    AnalysisLimits, LimitedWriter, hex, invalid,
    redaction::{self, RedactionProfile},
    schema::*,
};
use metaxtract_storage::{AuditAnchor, CaseInfo, CaseWorkspace, SearchQuery};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
};

#[derive(Clone, Copy, Debug)]
pub enum ExportFormat {
    Csv,
    Jsonl,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ExportReceipt {
    pub case_id: String,
    pub anchor: AuditAnchor,
    pub records: u64,
    pub bytes: u64,
    pub sha256: String,
    pub redacted: bool,
    pub case_integrity_verified: bool,
}
pub(crate) fn stamp(case: &CaseWorkspace) -> io::Result<(CaseInfo, AuditAnchor)> {
    let info = case.info()?;
    let integrity = case.verify_integrity(None)?;
    if case.info()?.revision != info.revision || case.audit_head()? != integrity.anchor {
        return Err(invalid("case changed while verifying export source"));
    }
    Ok((info, integrity.anchor))
}
pub(crate) fn unchanged(
    case: &CaseWorkspace,
    info: &CaseInfo,
    anchor: &AuditAnchor,
) -> io::Result<()> {
    if case.info()?.revision != info.revision || case.audit_head()? != *anchor {
        return Err(invalid(
            "case changed during export; output was not published",
        ));
    }
    Ok(())
}
pub(crate) fn atomic_output<T>(
    destination: &Path,
    write: impl FnOnce(&mut File) -> io::Result<T>,
) -> io::Result<(T, u64, String)> {
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let result = write(temp.as_file_mut())?;
    temp.as_file_mut().flush()?;
    temp.as_file().sync_all()?;
    temp.as_file_mut().seek(SeekFrom::Start(0))?;
    let mut sha = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let n = temp.as_file_mut().read(&mut buffer)?;
        if n == 0 {
            break;
        }
        sha.update(&buffer[..n]);
        bytes += n as u64;
    }
    temp.persist_noclobber(destination).map_err(|e| e.error)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok((result, bytes, hex(&sha.finalize())))
}
pub(crate) fn selected(
    case: &CaseWorkspace,
    query: &SearchQuery,
    limits: AnalysisLimits,
    mut consume: impl FnMut(u64, ExtractionRecord) -> io::Result<()>,
) -> io::Result<u64> {
    query.validate()?;
    let mut cursor = None;
    let mut count = 0u64;
    loop {
        let page = case.search_records(query, cursor.as_ref(), 128)?;
        for header in page.items {
            count += 1;
            if count > limits.max_records as u64 {
                return Err(invalid("selected record budget exceeded"));
            }
            let record = case
                .record(header.id)?
                .ok_or_else(|| invalid("selected record disappeared"))?;
            crate::check_record(&record, limits)?;
            consume(count, record)?;
        }
        if page.next.is_none() {
            break;
        }
        cursor = page.next;
    }
    Ok(count)
}
fn csv_cell(value: &str) -> String {
    let first = value
        .trim_start_matches(|c: char| c.is_whitespace() || c.is_control())
        .chars()
        .next();
    let guarded = if first.is_some_and(|c| matches!(c, '=' | '+' | '-' | '@')) {
        format!("'{value}")
    } else {
        value.into()
    };
    format!("\"{}\"", guarded.replace('"', "\"\""))
}
pub fn export_records(
    case: &CaseWorkspace,
    query: &SearchQuery,
    destination: impl AsRef<Path>,
    format: ExportFormat,
    profile: Option<&RedactionProfile>,
    limits: AnalysisLimits,
) -> io::Result<ExportReceipt> {
    if let Some(p) = profile {
        p.validate()?;
    }
    let (info, anchor) = stamp(case)?;
    let (records, bytes, sha256) = atomic_output(destination.as_ref(), |file| {
        let mut writer = LimitedWriter::new(file, limits.max_bytes);
        if matches!(format, ExportFormat::Csv) {
            writer.write_all(
                b"path,mime,size_bytes,status,sha256,claims_json,diagnostic_count\r\n",
            )?;
        }
        let count = selected(case, query, limits, |ordinal, record| {
            let record = if let Some(p) = profile {
                let alias = if p.hide_paths {
                    Some(EvidencePath::root(
                        RelativePath::parse(format!("evidence-{ordinal}.bin"))
                            .map_err(io::Error::other)?,
                    ))
                } else {
                    None
                };
                redaction::preview(&record, p, alias, limits)?.record
            } else {
                record
            };
            match format {
                ExportFormat::Jsonl => {
                    serde_json::to_writer(&mut writer, &record).map_err(io::Error::other)?;
                    writer.write_all(b"\n")?;
                }
                ExportFormat::Csv => {
                    let sha = record
                        .digests
                        .iter()
                        .find(|d| d.algorithm == DigestAlgorithm::Sha256)
                        .map(|d| hex(&d.bytes))
                        .unwrap_or_default();
                    let mime = record
                        .source
                        .detected_media_type
                        .as_deref()
                        .or(record.source.declared_media_type.as_deref())
                        .unwrap_or("application/octet-stream");
                    let claims = serde_json::to_string(&record.claims).map_err(io::Error::other)?;
                    let cells = [
                        record.source.path.to_string(),
                        mime.into(),
                        record.source.identity.size_bytes.to_string(),
                        format!("{:?}", record.status),
                        sha,
                        claims,
                        record.diagnostics.len().to_string(),
                    ];
                    writer.write_all(
                        cells
                            .iter()
                            .map(|v| csv_cell(v))
                            .collect::<Vec<_>>()
                            .join(",")
                            .as_bytes(),
                    )?;
                    writer.write_all(b"\r\n")?;
                }
            }
            Ok(())
        })?;
        writer.flush()?;
        unchanged(case, &info, &anchor)?;
        Ok(count)
    })?;
    Ok(ExportReceipt {
        case_id: info.case_id,
        anchor,
        records,
        bytes,
        sha256,
        redacted: profile.is_some(),
        case_integrity_verified: true,
    })
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceMember {
    path: EvidencePath,
    member: String,
    metadata_member: String,
    size_bytes: u64,
    sha256: String,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceManifest {
    format: String,
    case_id: String,
    anchor: AuditAnchor,
    original_evidence: bool,
    members: Vec<EvidenceMember>,
}
#[derive(serde::Serialize)]
pub struct EvidenceVerification {
    pub verified: bool,
    pub case_id: String,
    pub records: usize,
    pub bytes: u64,
}
/// Verify a bounded, stored selected-evidence ZIP without extracting any member.
pub fn verify_original_evidence(
    path: impl AsRef<Path>,
    limits: AnalysisLimits,
) -> io::Result<EvidenceVerification> {
    let input = File::open(path)?;
    if !input.metadata()?.is_file() || input.metadata()?.len() > limits.max_bytes as u64 {
        return Err(invalid("evidence archive exceeds byte budget"));
    }
    let mut archive = zip::ZipArchive::new(input).map_err(|e| invalid(&e.to_string()))?;
    if archive.len() > limits.max_records.saturating_mul(2).saturating_add(1) {
        return Err(invalid("evidence archive exceeds member budget"));
    }
    let mut inventory = std::collections::BTreeSet::new();
    let mut bytes = 0u64;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|e| invalid(&e.to_string()))?;
        bytes = bytes
            .checked_add(file.size())
            .ok_or_else(|| invalid("evidence archive size overflow"))?;
        if !inventory.insert(file.name().to_owned())
            || file.is_dir()
            || file.compression() != zip::CompressionMethod::Stored
            || file
                .unix_mode()
                .is_some_and(|m| m & 0o170000 != 0 && m & 0o170000 != 0o100000)
            || bytes > limits.max_bytes as u64
        {
            return Err(invalid(
                "ambiguous, special or oversized evidence archive member",
            ));
        }
    }
    let manifest: EvidenceManifest = {
        let file = archive
            .by_name("manifest.json")
            .map_err(|e| invalid(&e.to_string()))?;
        serde_json::from_reader(file).map_err(|e| invalid(&e.to_string()))?
    };
    if manifest.format != "metaxtract-selected-evidence-v1"
        || !manifest.original_evidence
        || manifest.members.len() > limits.max_records
        || archive.len() != manifest.members.len().saturating_mul(2).saturating_add(1)
    {
        return Err(invalid("unsupported evidence archive manifest"));
    }
    let mut expected = std::collections::BTreeSet::from(["manifest.json".to_owned()]);
    let mut buffer = [0u8; 65536];
    for (index, item) in manifest.members.iter().enumerate() {
        let ordinal = index + 1;
        if item.member != format!("evidence/{ordinal:08}.bin")
            || item.metadata_member != format!("metadata/{ordinal:08}.json")
        {
            return Err(invalid("non-canonical evidence archive member path"));
        }
        expected.insert(item.member.clone());
        expected.insert(item.metadata_member.clone());
        let record: ExtractionRecord = {
            let file = archive
                .by_name(&item.metadata_member)
                .map_err(|e| invalid(&e.to_string()))?;
            serde_json::from_reader(file).map_err(|e| invalid(&e.to_string()))?
        };
        crate::check_record(&record, limits)?;
        let digest = record
            .digests
            .iter()
            .find(|d| d.algorithm == DigestAlgorithm::Sha256 && d.bytes.len() == 32)
            .ok_or_else(|| invalid("evidence metadata has no SHA-256"))?;
        if record.source.path != item.path
            || record.source.identity.size_bytes != item.size_bytes
            || hex(&digest.bytes) != item.sha256
        {
            return Err(invalid("evidence metadata differs from the manifest"));
        }
        let mut file = archive
            .by_name(&item.member)
            .map_err(|e| invalid(&e.to_string()))?;
        if file.size() != item.size_bytes {
            return Err(invalid("evidence member size differs"));
        }
        let mut hash = Sha256::new();
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        if hex(&hash.finalize()) != item.sha256 {
            return Err(invalid("evidence member SHA-256 differs"));
        }
    }
    if inventory != expected {
        return Err(invalid("evidence archive contains unexpected members"));
    }
    Ok(EvidenceVerification {
        verified: true,
        case_id: manifest.case_id,
        records: manifest.members.len(),
        bytes,
    })
}
/// The caller supplies safely resolved, opened sources. Originals retain their embedded metadata.
pub fn export_original_evidence(
    case: &CaseWorkspace,
    query: &SearchQuery,
    destination: impl AsRef<Path>,
    limits: AnalysisLimits,
    mut open: impl FnMut(&ExtractionRecord) -> io::Result<Box<dyn Read>>,
) -> io::Result<ExportReceipt> {
    let (info, anchor) = stamp(case)?;
    let (records, bytes, sha256) = atomic_output(destination.as_ref(), |file| {
        let mut zip = zip::ZipWriter::new_stream(LimitedWriter::new(file, limits.max_bytes));
        let options = zip::write::SimpleFileOptions::default()
            .last_modified_time(zip::DateTime::default())
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(0o600)
            .large_file(true);
        let mut members = vec![];
        let mut input_bytes = 0u64;
        let count = selected(case, query, limits, |ordinal, record| {
            let expected = record
                .digests
                .iter()
                .find(|d| d.algorithm == DigestAlgorithm::Sha256 && d.bytes.len() == 32)
                .ok_or_else(|| invalid("original evidence export requires a recorded SHA-256"))?;
            let metadata_member = format!("metadata/{ordinal:08}.json");
            let member = format!("evidence/{ordinal:08}.bin");
            zip.start_file(&metadata_member, options)
                .map_err(io::Error::other)?;
            serde_json::to_writer(&mut zip, &record).map_err(io::Error::other)?;
            zip.start_file(&member, options).map_err(io::Error::other)?;
            let mut source = open(&record)?;
            let mut hasher = Sha256::new();
            let mut size = 0u64;
            let mut buffer = [0u8; 65536];
            loop {
                let n = source.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                size = size
                    .checked_add(n as u64)
                    .ok_or_else(|| invalid("evidence size overflow"))?;
                input_bytes = input_bytes.saturating_add(n as u64);
                if size > record.source.identity.size_bytes || input_bytes > limits.max_bytes as u64
                {
                    return Err(invalid(
                        "original evidence byte or recorded size budget exceeded",
                    ));
                }
                hasher.update(&buffer[..n]);
                zip.write_all(&buffer[..n])?;
            }
            let actual = hasher.finalize();
            if size != record.source.identity.size_bytes || actual.as_slice() != expected.bytes {
                return Err(invalid(
                    "evidence no longer matches its recorded size and SHA-256",
                ));
            }
            members.push(EvidenceMember {
                path: record.source.path,
                member,
                metadata_member,
                size_bytes: size,
                sha256: hex(&actual),
            });
            Ok(())
        })?;
        zip.start_file("manifest.json", options)
            .map_err(io::Error::other)?;
        serde_json::to_writer(
            &mut zip,
            &EvidenceManifest {
                format: "metaxtract-selected-evidence-v1".into(),
                case_id: info.case_id.clone(),
                anchor: anchor.clone(),
                original_evidence: true,
                members,
            },
        )
        .map_err(io::Error::other)?;
        zip.finish().map_err(io::Error::other)?.flush()?;
        unchanged(case, &info, &anchor)?;
        Ok(count)
    })?;
    Ok(ExportReceipt {
        case_id: info.case_id,
        anchor,
        records,
        bytes,
        sha256,
        redacted: false,
        case_integrity_verified: true,
    })
}
