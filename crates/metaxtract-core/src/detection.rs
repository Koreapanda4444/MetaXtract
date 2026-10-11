use metaxtract_native::{ContentCandidate, DetectionState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TypeConflictKind {
    ExtensionMismatch,
    MimeMismatch,
    ContainerMismatch,
    MultipleCandidates,
    InvalidHeader,
    TruncatedHeader,
    UnrecognizedContent,
    SubtypeUnverified,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeConflict {
    pub kind: TypeConflictKind,
    pub candidate_indices: Vec<usize>,
    pub declared: Option<String>,
    pub observed: Vec<String>,
    pub explanation: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentAssessment {
    pub candidates: Vec<ContentCandidate>,
    pub conflicts: Vec<TypeConflict>,
}

fn extension_family(extension: &str) -> Option<(&'static str, &'static [&'static str])> {
    Some(match extension {
        "jpg" | "jpeg" | "jpe" => ("jpeg", &["jpeg"]),
        "png" => ("png", &["png"]),
        "tif" | "tiff" => ("tiff", &["tiff", "bigtiff"]),
        "cr2" => ("tiff", &["cr2"]),
        "dng" | "nef" | "arw" => ("tiff", &[]),
        "webp" => ("riff", &["webp"]),
        "gif" => ("gif", &["gif"]),
        "bmp" => ("bmp", &["bmp"]),
        "ico" => ("ico", &["ico"]),
        "cur" => ("ico", &["cur"]),
        "heic" => ("isobmff", &["heic"]),
        "heif" => ("isobmff", &["heif", "heic"]),
        "avif" => ("isobmff", &["avif"]),
        "mp4" | "m4v" | "m4a" | "m4b" => ("isobmff", &["mp4"]),
        "mov" | "qt" => ("isobmff", &["mov"]),
        "wav" | "wave" => ("riff", &["wav"]),
        "avi" => ("riff", &["avi"]),
        "aif" | "aiff" | "aifc" => ("iff", &["aiff"]),
        "mp3" | "mp2" => ("mpeg_audio", &["mpeg_audio"]),
        "flac" => ("flac", &["flac"]),
        "ogg" | "oga" | "ogv" | "opus" => ("ogg", &[]),
        "ape" => ("ape", &["ape"]),
        "wv" => ("wavpack", &["wavpack"]),
        "mkv" | "mka" | "webm" => ("ebml", &[]),
        "mpg" | "mpeg" | "vob" => ("mpeg_ps", &["mpeg_ps"]),
        "ts" | "mts" | "m2ts" => ("mpeg_ts", &["mpeg_ts"]),
        "pdf" => ("pdf", &["pdf"]),
        "zip" => ("zip", &["zip"]),
        "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp" | "epub" => ("zip", &[]),
        "doc" | "xls" | "ppt" | "msg" => ("ole", &[]),
        "txt" | "json" | "csv" | "md" | "xml" | "html" => ("text", &["text"]),
        _ => return None,
    })
}

fn normalized_mime(mime: &str) -> String {
    let mime = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "image/jpg" | "image/pjpeg" => "image/jpeg".into(),
        "image/x-png" => "image/png".into(),
        "audio/x-wav" | "audio/wave" | "audio/vnd.wave" => "audio/wav".into(),
        "audio/x-aiff" => "audio/aiff".into(),
        "audio/x-flac" => "audio/flac".into(),
        "application/x-zip-compressed" => "application/zip".into(),
        _ => mime,
    }
}

pub fn assess_content(
    candidates: &[ContentCandidate],
    extension: Option<&str>,
    declared_mime: Option<&str>,
    declared_container: Option<&str>,
) -> ContentAssessment {
    let mut conflicts = Vec::new();
    let formats = || {
        candidates
            .iter()
            .map(|value| value.format.clone())
            .collect::<Vec<_>>()
    };
    let indices = || (0..candidates.len()).collect::<Vec<_>>();
    let mut report = |kind, declared, observed, explanation: &str| {
        conflicts.push(TypeConflict {
            kind,
            candidate_indices: indices(),
            declared,
            observed,
            explanation: explanation.into(),
        });
    };
    if candidates.is_empty() {
        report(
            TypeConflictKind::UnrecognizedContent,
            extension.map(str::to_owned),
            Vec::new(),
            "No recognized signature; the extension does not establish the content type",
        );
    }
    if candidates.len() > 1 {
        report(
            TypeConflictKind::MultipleCandidates,
            None,
            formats(),
            "Multiple signatures are present; preserve every candidate for structural analysis",
        );
    }
    if let Some(extension) = extension {
        let extension = extension.trim_start_matches('.').to_ascii_lowercase();
        if let Some((container, exact)) = extension_family(&extension) {
            let exact_match = candidates
                .iter()
                .any(|value| exact.contains(&value.format.as_str()));
            let container_match = candidates.iter().any(|value| value.container == container);
            let id3 = candidates.iter().any(|value| value.container == "id3");
            if !exact_match && (exact.is_empty() && container_match || id3) {
                report(
                    TypeConflictKind::SubtypeUnverified,
                    Some(extension),
                    formats(),
                    "The container or tag wrapper is recognized, but the declared subtype is unverified",
                );
            } else if !exact_match && !candidates.is_empty() {
                report(
                    TypeConflictKind::ExtensionMismatch,
                    Some(extension),
                    formats(),
                    "The extension differs from detected content; signature evidence is not full-file validation",
                );
            }
        }
    }
    if let Some(mime) = declared_mime {
        let mime = normalized_mime(mime);
        let generic =
            mime.is_empty() || mime == "application/octet-stream" || mime == "binary/octet-stream";
        let compatible = candidates.iter().any(|value| {
            mime == normalized_mime(&value.mime)
                || value.format == "mp4" && mime == "audio/mp4"
                || value.container == "ogg" && (mime == "video/ogg" || mime == "application/ogg")
                || value.container == "ebml" && (mime == "video/webm" || mime == "audio/webm")
                || value.container == "zip"
                    && (mime.starts_with("application/vnd.openxmlformats-")
                        || mime.starts_with("application/vnd.oasis.opendocument.")
                        || mime == "application/epub+zip")
        });
        if !generic && !compatible && !candidates.is_empty() {
            report(
                TypeConflictKind::MimeMismatch,
                Some(mime),
                candidates.iter().map(|value| value.mime.clone()).collect(),
                "Declared MIME differs from signature-derived candidates",
            );
        }
    }
    if let Some(container) = declared_container {
        let container = container.trim().to_ascii_lowercase();
        if !container.is_empty()
            && !candidates.is_empty()
            && !candidates.iter().any(|value| value.container == container)
        {
            report(
                TypeConflictKind::ContainerMismatch,
                Some(container),
                candidates
                    .iter()
                    .map(|value| value.container.clone())
                    .collect(),
                "Declared container differs from signature-derived containers",
            );
        }
    }
    for (index, value) in candidates.iter().enumerate() {
        let kind = match value.state {
            DetectionState::Invalid => TypeConflictKind::InvalidHeader,
            DetectionState::Truncated => TypeConflictKind::TruncatedHeader,
            _ => continue,
        };
        conflicts.push(TypeConflict {
            kind,
            candidate_indices: vec![index],
            declared: None,
            observed: vec![value.format.clone()],
            explanation: value.detail.clone(),
        });
    }
    ContentAssessment {
        candidates: candidates.to_vec(),
        conflicts,
    }
}

#[cfg(test)]
mod checks {
    use super::*;
    use metaxtract_native::{BoundedInput, InputLimits, ParserLimits, detect_content};

    fn inspect(bytes: Vec<u8>, extension: &str, mime: &str, container: &str) -> ContentAssessment {
        let input = BoundedInput::from_bytes(bytes, InputLimits::default()).unwrap();
        let candidates = detect_content(&input, ParserLimits::default()).unwrap();
        assess_content(&candidates, Some(extension), Some(mime), Some(container))
    }

    #[test]
    fn reconciles_independent_type_evidence_without_promoting_a_filename_to_truth() {
        let renamed = inspect(vec![255, 216, 255], "txt", "text/plain", "zip");
        assert_eq!(renamed.candidates[0].format, "jpeg");
        assert!(
            renamed
                .conflicts
                .iter()
                .any(|value| value.kind == TypeConflictKind::ExtensionMismatch)
        );
        assert!(
            renamed
                .conflicts
                .iter()
                .any(|value| value.kind == TypeConflictKind::MimeMismatch)
        );
        assert!(
            renamed
                .conflicts
                .iter()
                .any(|value| value.kind == TypeConflictKind::ContainerMismatch)
        );
        let alias = inspect(
            vec![255, 216, 255],
            ".JPEG",
            "image/jpg; charset=binary",
            "jpeg",
        );
        assert!(alias.conflicts.is_empty());
        let packaged = inspect(
            vec![80, 75, 3, 4],
            "docx",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "zip",
        );
        assert_eq!(packaged.conflicts.len(), 1);
        assert_eq!(
            packaged.conflicts[0].kind,
            TypeConflictKind::SubtypeUnverified
        );
        let unknown = inspect(b"unrecognized".to_vec(), "png", "image/png", "png");
        assert_eq!(
            unknown.conflicts[0].kind,
            TypeConflictKind::UnrecognizedContent
        );
        let truncated = inspect(
            vec![137, 80, 78, 71, 13, 10, 26, 10],
            "png",
            "image/png",
            "png",
        );
        assert_eq!(
            truncated.conflicts[0].kind,
            TypeConflictKind::TruncatedHeader
        );
        let polyglot = inspect(
            b"PK\x03\x04xx%PDF-1.7".to_vec(),
            "pdf",
            "application/pdf",
            "pdf",
        );
        assert_eq!(polyglot.candidates.len(), 2);
        assert_eq!(
            polyglot.conflicts[0].kind,
            TypeConflictKind::MultipleCandidates
        );
    }
}
