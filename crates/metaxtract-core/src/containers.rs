use std::{
    fs::File,
    io,
    time::{Duration, Instant},
};

use metaxtract_native::{
    BoundedInput, ExtractionResult, InputLimits, NativeError, NativeErrorCode, ParserLimits,
    ParserRun, extract_parser, parser_capabilities,
};
use metaxtract_schema::{EvidenceKind, RawValue};

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug)]
pub struct RecursiveLimits {
    #[serde(default)]
    pub preview: bool,
    pub parser: ParserLimits,
    pub max_depth: u32,
    pub max_nodes: usize,
    pub max_member_bytes: u64,
    pub max_total_input_bytes: u64,
    pub max_wall_time_ms: u32,
}

impl Default for RecursiveLimits {
    fn default() -> Self {
        Self {
            preview: false,
            parser: ParserLimits::default(),
            max_depth: 16,
            max_nodes: 4096,
            max_member_bytes: 64 * 1024 * 1024,
            max_total_input_bytes: 256 * 1024 * 1024,
            max_wall_time_ms: 90_000,
        }
    }
}

#[derive(Clone, Debug)]
pub struct NestedIdentity {
    pub parent: usize,
    pub ordinal: u32,
    pub kind: EvidenceKind,
    pub name: Vec<u8>,
    pub packet_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RecursiveNode {
    pub identity: Option<NestedIdentity>,
    pub depth: u32,
    pub size_bytes: u64,
    pub results: Vec<ExtractionResult>,
}

#[derive(Clone, Debug)]
pub struct RecursiveExtraction {
    pub nodes: Vec<RecursiveNode>,
    pub usage: ParserRun,
    pub input_bytes: u64,
    pub stopped: Option<NativeErrorCode>,
}

pub(crate) fn selected_parsers(bytes: &[u8], name: &[u8]) -> Vec<&'static str> {
    let mut result = Vec::new();
    let lower = String::from_utf8_lossy(name).to_ascii_lowercase();
    let archive = bytes.starts_with(b"PK")
        || bytes.starts_with(b"7z\xbc\xaf\x27\x1c")
        || bytes.starts_with(b"\x1f\x8b")
        || bytes.starts_with(b"BZh")
        || bytes.starts_with(b"\xfd7zXZ\0")
        || bytes.get(257..262) == Some(b"ustar")
        || [".tar", ".lzma", ".z"]
            .iter()
            .any(|ext| lower.ends_with(ext));
    if archive {
        result.push("container.archive");
    }
    if bytes.starts_with(b"PK") {
        result.push("document.package");
        result.push("formats.resource");
    }
    if bytes.starts_with(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1") {
        result.push("document.ole");
    }
    if bytes.starts_with(b"%PDF-") {
        result.extend(["pdf.metadata", "pdf.security"]);
    }
    if bytes.starts_with(b"\x89PNG")
        || bytes.starts_with(b"\xff\xd8")
        || bytes.starts_with(b"II")
        || bytes.starts_with(b"MM")
        || bytes.starts_with(b"GIF8")
        || bytes.starts_with(b"BM")
        || bytes.get(8..12) == Some(b"WEBP")
        || bytes.starts_with(b"\0\0\x01\0")
    {
        result.extend(["images.structure", "images.metadata"]);
    }
    if bytes.get(4..8) == Some(b"ftyp") {
        result.extend(["images.heif", "media.structure", "media.inspect"]);
    }
    if bytes.starts_with(b"II")
        || bytes.starts_with(b"MM")
        || bytes.starts_with(b"FUJIFILMCCD-RAW")
        || bytes.get(6..14) == Some(b"HEAPCCDR")
        || [
            ".cr2", ".cr3", ".crw", ".nef", ".nrw", ".arw", ".dng", ".raf", ".rw2", ".orf", ".pef",
            ".srw",
        ]
        .iter()
        .any(|ext| lower.ends_with(ext))
    {
        result.push("images.raw");
    }
    if bytes.starts_with(b"ID3")
        || bytes.starts_with(b"fLaC")
        || bytes.starts_with(b"OggS")
        || bytes.get(8..12) == Some(b"WAVE")
        || bytes.starts_with(b"FORM")
    {
        result.extend(["audio.metadata", "media.inspect"]);
    }
    if bytes.starts_with(b"RIFF")
        || bytes.starts_with(b"\x1a\x45\xdf\xa3")
        || bytes.starts_with(b"\0\0\x01")
        || bytes.starts_with(b"\xff")
        || [
            ".avi", ".mkv", ".mka", ".webm", ".mp4", ".mov", ".m4a", ".mp3", ".mp2", ".mpg",
            ".mpeg", ".ts", ".m2ts", ".mts", ".ape", ".wv",
        ]
        .iter()
        .any(|ext| lower.ends_with(ext))
    {
        result.extend(["media.structure", "media.inspect"]);
    }
    if result
        .iter()
        .any(|id| id.starts_with("media.") || *id == "audio.metadata")
    {
        result.push("media.details");
    }
    if lower.ends_with(".eml")
        || lower.ends_with(".mime")
        || bytes.starts_with(b"From:")
        || bytes.starts_with(b"Subject:")
        || bytes.starts_with(b"Received:")
    {
        result.push("email.mime");
    }
    if bytes.starts_with(b"MZ")
        || bytes.starts_with(b"\x7fELF")
        || bytes.starts_with(b"#!")
        || bytes.starts_with(b"\xca\xfe\xba\xbe")
        || bytes.starts_with(b"\xca\xfe\xba\xbf")
        || bytes.starts_with(b"\xbe\xba\xfe\xca")
        || bytes.starts_with(b"\xbf\xba\xfe\xca")
        || bytes.starts_with(b"\xfe\xed\xfa")
        || bytes.starts_with(b"\xcf\xfa\xed\xfe")
        || bytes.starts_with(b"\xce\xfa\xed\xfe")
        || [".py", ".sh", ".ps1", ".js", ".bat", ".cmd"]
            .iter()
            .any(|ext| lower.ends_with(ext))
    {
        result.push("binaries.metadata");
    }
    if bytes.starts_with(b"\0\x01\0\0")
        || bytes.starts_with(b"OTTO")
        || bytes.starts_with(b"ttcf")
        || bytes.starts_with(b"wOFF")
        || bytes.starts_with(b"wOF2")
        || bytes.starts_with(b"true")
        || bytes.starts_with(b"typ1")
        || bytes.get(60..68) == Some(b"BOOKMOBI")
    {
        result.push("formats.resource");
    }
    if bytes.get(128..132) == Some(b"DICM") || lower.ends_with(".dcm") {
        result.push("formats.dicom");
    }
    if bytes.starts_with(b"II")
        || bytes.starts_with(b"MM")
        || [".geojson", ".gpx", ".kml", ".shp", ".dbf", ".tif", ".tiff"]
            .iter()
            .any(|ext| lower.ends_with(ext))
    {
        result.push("formats.geo");
    }
    result.sort_unstable();
    result.dedup();
    result
}

fn payload_kind(parser: &str, key: &str) -> Option<EvidenceKind> {
    match (parser, key) {
        ("container.archive", "member_payload") => Some(EvidenceKind::ArchiveMember),
        ("document.ole", "embedded_payload") => Some(EvidenceKind::EmbeddedObject),
        ("document.ole", "stream_payload") => Some(EvidenceKind::AlternateStream),
        ("email.mime", "body_payload") => Some(EvidenceKind::Attachment),
        ("pdf.metadata", "attachment_data") => Some(EvidenceKind::Attachment),
        _ => None,
    }
}

fn zero_usage() -> ParserRun {
    ParserRun {
        read_bytes: 0,
        allocation_bytes: 0,
        decoded_bytes: 0,
        entries: 0,
        observations: 0,
    }
}

fn capacity_error(code: NativeErrorCode) -> NativeError {
    NativeError {
        code,
        recoverable: true,
        message: "Recursive extraction resource limit reached".into(),
    }
}

pub fn extract_recursive(
    bytes: Vec<u8>,
    name: Vec<u8>,
    limits: RecursiveLimits,
) -> Result<RecursiveExtraction, NativeError> {
    extract_input(WorkInput::Bytes(bytes), name, limits, &|| Ok(()))
}

pub fn extract_file_recursive(
    file: File,
    prefix: Vec<u8>,
    name: Vec<u8>,
    limits: RecursiveLimits,
) -> Result<RecursiveExtraction, NativeError> {
    extract_file_recursive_controlled(file, prefix, name, limits, || Ok(()))
}

pub fn extract_file_recursive_controlled(
    file: File,
    prefix: Vec<u8>,
    name: Vec<u8>,
    limits: RecursiveLimits,
    checkpoint: impl Fn() -> io::Result<()>,
) -> Result<RecursiveExtraction, NativeError> {
    let size = file
        .metadata()
        .map_err(|_| capacity_error(NativeErrorCode::ReadFailure))?
        .len();
    if size > limits.parser.max_source_bytes {
        return Err(capacity_error(NativeErrorCode::SourceTooLarge));
    }
    extract_input(
        WorkInput::File(file, prefix, size),
        name,
        limits,
        &checkpoint,
    )
}

enum WorkInput {
    Bytes(Vec<u8>),
    File(File, Vec<u8>, u64),
}
impl WorkInput {
    fn size(&self) -> u64 {
        match self {
            Self::Bytes(bytes) => bytes.len() as u64,
            Self::File(_, _, size) => *size,
        }
    }
    fn prefix(&self) -> &[u8] {
        match self {
            Self::Bytes(bytes) => bytes,
            Self::File(_, prefix, _) => prefix,
        }
    }
}
fn extract_input(
    input: WorkInput,
    name: Vec<u8>,
    limits: RecursiveLimits,
    checkpoint: &impl Fn() -> io::Result<()>,
) -> Result<RecursiveExtraction, NativeError> {
    if limits.max_nodes == 0
        || limits.max_wall_time_ms == 0
        || limits.max_member_bytes == 0
        || limits.parser.max_wall_time_ms == 0
    {
        return Err(capacity_error(NativeErrorCode::InvalidRequest));
    }
    if matches!(&input, WorkInput::Bytes(_))
        && (input.size() > limits.max_member_bytes || input.size() > limits.max_total_input_bytes)
    {
        return Err(capacity_error(NativeErrorCode::SourceTooLarge));
    }
    let root_expansion_limit = if limits.parser.max_expansion_ratio == 0 {
        u64::MAX
    } else {
        input
            .size()
            .saturating_mul(u64::from(limits.parser.max_expansion_ratio))
    };
    let decoded_limit = limits.parser.max_decoded_bytes.min(root_expansion_limit);
    let member_limit = limits.max_member_bytes.min(limits.parser.max_member_bytes);
    let capabilities = parser_capabilities()?;
    let deadline = Instant::now() + Duration::from_millis(u64::from(limits.max_wall_time_ms));
    struct Work {
        input: WorkInput,
        name: Vec<u8>,
        identity: Option<NestedIdentity>,
        depth: u32,
    }
    let mut pending = vec![Work {
        input,
        name,
        identity: None,
        depth: 0,
    }];
    let mut output = RecursiveExtraction {
        nodes: Vec::new(),
        usage: zero_usage(),
        input_bytes: 0,
        stopped: None,
    };
    let mut queued_bytes = if matches!(&pending[0].input, WorkInput::Bytes(_)) {
        pending[0].input.size()
    } else {
        0
    };
    while let Some(work) = pending.pop() {
        checkpoint().map_err(|e| {
            capacity_error(if e.kind() == io::ErrorKind::TimedOut {
                NativeErrorCode::DeadlineExceeded
            } else {
                NativeErrorCode::ReadFailure
            })
        })?;
        if Instant::now() >= deadline {
            output.stopped = Some(NativeErrorCode::DeadlineExceeded);
            break;
        }
        if work.depth > limits.max_depth {
            output.stopped = Some(NativeErrorCode::ContainerDepthExceeded);
            continue;
        }
        if output.nodes.len() >= limits.max_nodes {
            output.stopped = Some(NativeErrorCode::EntryLimitExceeded);
            break;
        }
        let size = work.input.size();
        let file_root = matches!(&work.input, WorkInput::File(..));
        if (!file_root && size > limits.max_member_bytes)
            || queued_bytes > limits.max_total_input_bytes
        {
            output.stopped = Some(NativeErrorCode::SourceTooLarge);
            continue;
        }
        output.input_bytes = output.input_bytes.saturating_add(size);
        let mut parsers = selected_parsers(work.input.prefix(), &work.name);
        if limits.preview {
            if parsers.iter().any(|p| {
                p.starts_with("images.") || p.starts_with("media.") || p.starts_with("audio.")
            }) {
                parsers.push("media.decode");
            }
            if parsers.contains(&"pdf.metadata") {
                parsers.push("pdf.preview");
            }
            if parsers.contains(&"document.package") {
                parsers.push("document.preview");
            }
        }
        let input_limits = InputLimits {
            max_source_bytes: if file_root {
                limits.parser.max_source_bytes
            } else {
                limits.max_member_bytes.min(limits.parser.max_source_bytes)
            },
            max_single_read: 1024 * 1024,
            max_total_read: limits.parser.max_read_bytes.max(1),
        };
        let input = match work.input {
            WorkInput::Bytes(bytes) => BoundedInput::from_bytes(bytes, input_limits)?,
            WorkInput::File(file, _, _) => BoundedInput::from_file(file, input_limits)?,
        };
        let index = output.nodes.len();
        let mut node = RecursiveNode {
            identity: work.identity,
            depth: work.depth,
            size_bytes: size,
            results: Vec::new(),
        };
        for id in parsers {
            checkpoint().map_err(|e| {
                capacity_error(if e.kind() == io::ErrorKind::TimedOut {
                    NativeErrorCode::DeadlineExceeded
                } else {
                    NativeErrorCode::ReadFailure
                })
            })?;
            if !capabilities.iter().any(|cap| cap.id == id) {
                continue;
            }
            let remaining_ms = deadline
                .saturating_duration_since(Instant::now())
                .as_millis();
            if remaining_ms == 0 {
                output.stopped = Some(NativeErrorCode::DeadlineExceeded);
                break;
            }
            let tree_remaining = limits.max_total_input_bytes.saturating_sub(queued_bytes);
            let available = ParserLimits {
                max_source_bytes: if file_root {
                    limits.parser.max_source_bytes
                } else {
                    limits.parser.max_source_bytes.min(limits.max_member_bytes)
                },
                max_read_bytes: limits
                    .parser
                    .max_read_bytes
                    .saturating_sub(output.usage.read_bytes),
                max_allocation_bytes: limits
                    .parser
                    .max_allocation_bytes
                    .saturating_sub(output.usage.allocation_bytes),
                max_decoded_bytes: decoded_limit.saturating_sub(output.usage.decoded_bytes),
                max_member_bytes: member_limit.min(tree_remaining),
                max_expansion_ratio: limits.parser.max_expansion_ratio,
                max_entries: limits
                    .parser
                    .max_entries
                    .saturating_sub(output.usage.entries),
                max_observations: limits
                    .parser
                    .max_observations
                    .saturating_sub(output.usage.observations),
                max_container_depth: limits.parser.max_container_depth.saturating_sub(work.depth),
                max_wall_time_ms: limits
                    .parser
                    .max_wall_time_ms
                    .min(remaining_ms.min(u128::from(u32::MAX)) as u32),
            };
            let mut result = extract_parser(&input, id, available)?;
            if let Some(error) = &mut result.error {
                if error.code == NativeErrorCode::DecodeBudgetExceeded
                    && root_expansion_limit < limits.parser.max_decoded_bytes
                {
                    *error = capacity_error(NativeErrorCode::ExpansionRatioExceeded);
                } else if error.code == NativeErrorCode::MemberSizeExceeded
                    && tree_remaining < member_limit
                {
                    *error = capacity_error(NativeErrorCode::DecodeBudgetExceeded);
                }
            }
            output.usage.read_bytes = output
                .usage
                .read_bytes
                .saturating_add(result.run.read_bytes);
            output.usage.allocation_bytes = output
                .usage
                .allocation_bytes
                .saturating_add(result.run.allocation_bytes);
            output.usage.decoded_bytes = output
                .usage
                .decoded_bytes
                .saturating_add(result.run.decoded_bytes);
            output.usage.entries = output.usage.entries.saturating_add(result.run.entries);
            output.usage.observations = output
                .usage
                .observations
                .saturating_add(result.run.observations);
            if let Some(error) = &result.error {
                if matches!(
                    error.code,
                    NativeErrorCode::ReadBudgetExceeded
                        | NativeErrorCode::MemberSizeExceeded
                        | NativeErrorCode::AllocationBudgetExceeded
                        | NativeErrorCode::DecodeBudgetExceeded
                        | NativeErrorCode::ExpansionRatioExceeded
                        | NativeErrorCode::EntryLimitExceeded
                        | NativeErrorCode::ObservationLimitExceeded
                        | NativeErrorCode::DeadlineExceeded
                ) {
                    output.stopped = Some(error.code);
                }
            }
            for (ordinal, observation) in result.observations.iter().enumerate() {
                let Some(mut kind) = payload_kind(id, &observation.key) else {
                    continue;
                };
                if id == "email.mime" {
                    let part = observation.namespace.trim_end_matches(".body.decoded");
                    let attachment = result.observations.iter().any(|field| {
                        field.namespace == part
                            && field.key == "attachment"
                            && field.value == RawValue::Unsigned(1)
                    });
                    if !attachment {
                        kind = EvidenceKind::Resource;
                    }
                }
                let RawValue::Bytes(payload) = &observation.value else {
                    continue;
                };
                if work.depth >= limits.max_depth {
                    output.stopped = Some(NativeErrorCode::ContainerDepthExceeded);
                    continue;
                }
                if pending
                    .len()
                    .saturating_add(output.nodes.len())
                    .saturating_add(1)
                    >= limits.max_nodes
                {
                    output.stopped = Some(NativeErrorCode::EntryLimitExceeded);
                    continue;
                }
                let child_size = payload.len() as u64;
                let charge = child_size.saturating_mul(2).saturating_add(4096);
                if child_size > member_limit {
                    output.stopped = Some(NativeErrorCode::MemberSizeExceeded);
                    continue;
                }
                if queued_bytes.saturating_add(child_size) > limits.max_total_input_bytes {
                    output.stopped = Some(NativeErrorCode::DecodeBudgetExceeded);
                    continue;
                }
                if output.usage.allocation_bytes.saturating_add(charge)
                    > limits.parser.max_allocation_bytes
                {
                    output.stopped = Some(NativeErrorCode::AllocationBudgetExceeded);
                    continue;
                }
                let member = result.observations.iter().find(|field| {
                    field.namespace == observation.namespace.trim_end_matches(".body.decoded")
                        && matches!(
                            field.key.as_str(),
                            "filename"
                                | "filename_utf8"
                                | "part_name"
                                | "attachment_filename"
                                | "storage_path"
                        )
                });
                let member_ordinal = result
                    .observations
                    .iter()
                    .find_map(|field| {
                        if field.namespace
                            != observation.namespace.trim_end_matches(".body.decoded")
                            || !matches!(
                                field.key.as_str(),
                                "member_index" | "part_index" | "stream_index"
                            )
                        {
                            return None;
                        }
                        match field.value {
                            RawValue::Unsigned(value) => u32::try_from(value).ok(),
                            _ => None,
                        }
                    })
                    .unwrap_or(ordinal as u32);
                let name = member
                    .map(|field| match &field.value {
                        RawValue::Bytes(bytes) => bytes.clone(),
                        RawValue::Text(text) => text.text.as_bytes().to_vec(),
                        _ => Vec::new(),
                    })
                    .unwrap_or_default();
                let mut data = Vec::new();
                data.try_reserve_exact(payload.len())
                    .map_err(|_| capacity_error(NativeErrorCode::AllocationFailure))?;
                data.extend_from_slice(payload);
                output.usage.allocation_bytes += charge;
                queued_bytes += child_size;
                pending.push(Work {
                    input: WorkInput::Bytes(data),
                    name: name.clone(),
                    depth: work.depth + 1,
                    identity: Some(NestedIdentity {
                        parent: index,
                        ordinal: member_ordinal,
                        kind,
                        name,
                        packet_id: observation.stream_id.clone(),
                    }),
                });
            }
            node.results.push(result);
            if output.stopped.is_some_and(|code| {
                matches!(
                    code,
                    NativeErrorCode::DeadlineExceeded
                        | NativeErrorCode::MemberSizeExceeded
                        | NativeErrorCode::ReadBudgetExceeded
                        | NativeErrorCode::AllocationBudgetExceeded
                        | NativeErrorCode::DecodeBudgetExceeded
                        | NativeErrorCode::ExpansionRatioExceeded
                        | NativeErrorCode::EntryLimitExceeded
                        | NativeErrorCode::ObservationLimitExceeded
                )
            }) {
                break;
            }
        }
        output.nodes.push(node);
        if output.stopped.is_some_and(|code| {
            matches!(
                code,
                NativeErrorCode::DeadlineExceeded
                    | NativeErrorCode::ReadBudgetExceeded
                    | NativeErrorCode::AllocationBudgetExceeded
                    | NativeErrorCode::DecodeBudgetExceeded
                    | NativeErrorCode::ExpansionRatioExceeded
                    | NativeErrorCode::ObservationLimitExceeded
            )
        }) {
            break;
        }
    }
    Ok(output)
}
