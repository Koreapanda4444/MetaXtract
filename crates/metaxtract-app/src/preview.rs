//! Evidence previews use an explicitly chosen source, a verified snapshot and the bundled worker.
use crate::{core::scan::ScanControl, schema::*};
use sha2::{Digest, Sha256};
use std::{io, path::Path};
const MAX_SOURCE: usize = 64 * 1024 * 1024;
const MAX_TEXT: usize = 32768;
#[derive(Clone, Debug, serde::Serialize)]
pub struct Preview {
    pub verified: bool,
    pub bytes: u64,
    pub text: String,
    pub hex: Vec<String>,
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
    pub notices: Vec<String>,
}
fn name(o: &MetadataObservation) -> &str {
    o.tag.name.as_deref().unwrap_or("")
}
fn raw(o: &MetadataObservation) -> &RawValue {
    o.interpreted_value.as_ref().unwrap_or(&o.raw_value)
}
fn text(v: &RawValue) -> Option<&str> {
    if let RawValue::Text(t) = v {
        Some(t.text.as_str())
    } else {
        None
    }
}
fn number(v: &RawValue) -> Option<usize> {
    match v {
        RawValue::Unsigned(n) => usize::try_from(*n).ok(),
        RawValue::Signed(n) => usize::try_from(*n).ok(),
        _ => None,
    }
}
fn push(out: &mut String, s: &str) {
    for c in s.chars() {
        if out.len() + c.len_utf8() > MAX_TEXT {
            return;
        }
        out.push(c);
    }
    if out.len() < MAX_TEXT {
        out.push('\n');
    }
}
pub fn load(record: &ExtractionRecord, path: &Path) -> io::Result<Preview> {
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_SOURCE as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "preview requires a regular file of at most 64 MiB",
        ));
    }
    let expected = record
        .digests
        .iter()
        .find(|d| d.algorithm == DigestAlgorithm::Sha256 && d.bytes.len() == 32)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "preview requires the recorded SHA-256",
            )
        })?;
    let bytes = crate::read_bytes(path, MAX_SOURCE)?;
    if bytes.len() as u64 != record.source.identity.size_bytes
        || Sha256::digest(&bytes).as_slice() != expected.bytes
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "selected source does not match the recorded size and SHA-256",
        ));
    }
    let mut preview=Preview{verified:true,bytes:bytes.len()as u64,text:String::new(),hex:vec![],width:0,height:0,rgb:vec![],notices:vec!["Source size and SHA-256 verified / 원본 크기·SHA-256 일치".into(),"Bounded preview; originals and case metadata remain unchanged. / 제한된 미리보기이며 원본과 케이스 메타데이터를 변경하지 않습니다.".into()]};
    for (line, chunk) in bytes[..bytes.len().min(65536)].chunks(16).enumerate() {
        preview.hex.push(format!(
            "{:08x}  {:47}  {}",
            line * 16,
            chunk
                .iter()
                .map(|n| format!("{n:02x}"))
                .collect::<Vec<_>>()
                .join(" "),
            chunk
                .iter()
                .map(|n| if (32..127).contains(n) {
                    char::from(*n)
                } else {
                    '.'
                })
                .collect::<String>()
        ));
    }
    if bytes.len() > 65536 {
        preview
            .notices
            .push("Hex shows the first 64 KiB / 16진수는 처음 64 KiB만 표시".into());
    }
    let mime = record
        .source
        .detected_media_type
        .as_deref()
        .or(record.source.declared_media_type.as_deref())
        .unwrap_or("");
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if mime.starts_with("text/")
        || [
            "txt", "md", "log", "json", "xml", "csv", "yaml", "yml", "toml", "ini", "py", "sh",
            "rs", "cpp",
        ]
        .contains(&extension.as_str())
    {
        if let Ok(s) = std::str::from_utf8(&bytes) {
            if !s.contains('\0') {
                push(&mut preview.text, s);
            }
        }
    }
    let directory = tempfile::tempdir()?;
    let mut filename = std::ffi::OsString::from("evidence");
    if let Some(ext) = path.extension() {
        filename.push(".");
        filename.push(ext);
    }
    let source = directory.path().join(filename);
    std::fs::write(&source, &bytes)?;
    drop(bytes);
    let case_path = directory.path().join("preview.mxcase");
    let mut request = crate::scan::ScanRequest::new(source, case_path.clone());
    request.workers = 1;
    request.options.scheduling_window = 1;
    request.options.queue_capacity = 1;
    request.options.recursive.preview = true;
    request.options.recursive.max_depth = 0;
    request.options.recursive.max_nodes = 1;
    request.options.recursive.max_wall_time_ms = 15000;
    request.options.recursive.parser.max_wall_time_ms = 10000;
    request.options.file_wall_time_ms = 20000;
    request.options.recursive.parser.max_source_bytes = MAX_SOURCE as u64;
    request.options.recursive.max_total_input_bytes = MAX_SOURCE as u64;
    request.options.recursive.parser.max_allocation_bytes = 128 * 1024 * 1024;
    request.options.recursive.parser.max_decoded_bytes = 32 * 1024 * 1024;
    request.options.recursive.parser.max_entries = 10000;
    request.options.recursive.parser.max_observations = 30000;
    request.options.max_result_bytes = 32 * 1024 * 1024;
    request.options.max_memory_bytes = 512 * 1024 * 1024;
    let outcome = match crate::scan::run(&request, ScanControl::new(), |_| Ok(())) {
        Ok(out) => out,
        Err(error) => {
            preview.notices.push(format!(
                "Native preview unavailable / 네이티브 미리보기 사용 불가: {error}"
            ));
            return Ok(preview);
        }
    };
    let case = crate::open_case(&case_path, false)?;
    preview.notices.extend(
        outcome
            .errors
            .iter()
            .take(8)
            .map(|e| format!("{}: {}", e.path, e.message)),
    );
    let records = crate::collect(
        &case,
        &crate::analysis::search::parse(&outcome.scan_id, "")?,
    )?;
    for record in records {
        for o in &record.observations {
            if name(o) == "preview_pixels" && preview.rgb.is_empty() {
                let find = |field: &str| {
                    record.observations.iter().find(|p| {
                        name(p) == field
                            && p.tag.namespace == o.tag.namespace
                            && p.provenance.parser.id == o.provenance.parser.id
                            && p.provenance.stream_id == o.provenance.stream_id
                    })
                };
                let width = find("preview_width")
                    .and_then(|p| number(raw(p)))
                    .unwrap_or(0);
                let height = find("preview_height")
                    .and_then(|p| number(raw(p)))
                    .unwrap_or(0);
                if let RawValue::Bytes(rgb) = raw(o) {
                    if width > 0
                        && height > 0
                        && width <= 4096
                        && height <= 4096
                        && width.checked_mul(height).and_then(|n| n.checked_mul(3))
                            == Some(rgb.len())
                        && rgb.len() <= 8 * 1024 * 1024
                        && find("preview_format").and_then(|p| text(raw(p))) == Some("RGB24")
                    {
                        preview.width = width;
                        preview.height = height;
                        preview.rgb = rgb.clone();
                    }
                }
            }
            if name(o) == "preview_text" {
                if let Some(s) = text(raw(o)) {
                    push(&mut preview.text, s);
                }
            }
            if name(o) == "preview_note" {
                if let Some(s) = text(raw(o)) {
                    preview.notices.push(s.to_owned());
                }
            }
            if name(o).starts_with("sample_")
                || name(o).starts_with("frame_")
                || name(o).starts_with("decoded_")
            {
                let rendered = match raw(o) {
                    RawValue::Text(t) => Some(t.text.clone()),
                    RawValue::Unsigned(n) => Some(n.to_string()),
                    RawValue::Signed(n) => Some(n.to_string()),
                    _ => None,
                };
                if let Some(s) = rendered {
                    push(&mut preview.text, &format!("{}: {s}", name(o)));
                }
            }
        }
        for diagnostic in record.diagnostics.iter().take(12) {
            preview
                .notices
                .push(format!("{}: {}", diagnostic.code, diagnostic.message));
        }
    }
    if preview.text.len() >= MAX_TEXT {
        preview
            .notices
            .push("Text preview reached 32 KiB / 텍스트 미리보기 32 KiB 한도 도달".into());
    }
    if preview.rgb.is_empty() {
        preview.notices.push("No supported RGB preview was available; text/hex remain available. / 지원되는 RGB 미리보기가 없어 텍스트·16진수로 표시합니다.".into());
    }
    if !preview.rgb.is_empty() {
        preview.notices.push("First decoded frame; embedded display transforms may remain unapplied. / 첫 디코딩 프레임이며 내장된 표시 변환이 적용되지 않을 수 있습니다.".into());
    }
    preview.notices.truncate(40);
    Ok(preview)
}
