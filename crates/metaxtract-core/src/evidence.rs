use metaxtract_native::{NativeError, NativeErrorCode, parser_capabilities};
use metaxtract_schema::{
    Diagnostic, DiagnosticSeverity, DiagnosticStage, EvidenceKind, EvidencePath, EvidenceSegment,
    ExtractionRecord, ExtractionStatus, FileIdentity, ObservationId, ParserDescriptor,
    RelativePath, SourceDescriptor, SourceKind,
};

use crate::{BackendClaimLimits, RecursiveExtraction, independent_backend_claims};

fn member_path(bytes: &[u8]) -> RelativePath {
    if let Ok(text) = std::str::from_utf8(bytes) {
        if let Ok(path) = RelativePath::parse(text) {
            return path;
        }
    }
    // Invalid names remain reversible without accepting traversal or control characters.
    let mut encoded = String::from("~raw-");
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(encoded, "{byte:02x}");
    }
    RelativePath::parse(encoded).expect("hexadecimal names are safe relative paths")
}

fn invalid_tree() -> NativeError {
    NativeError {
        code: NativeErrorCode::InvalidRequest,
        message: "Nested evidence parent must precede its child".into(),
        recoverable: false,
    }
}

impl RecursiveExtraction {
    pub fn evidence_paths(&self, root: RelativePath) -> Result<Vec<EvidencePath>, NativeError> {
        let mut paths: Vec<EvidencePath> = Vec::with_capacity(self.nodes.len());
        for (index, node) in self.nodes.iter().enumerate() {
            let path = match &node.identity {
                None if index == 0 => EvidencePath::root(root.clone()),
                Some(identity) if identity.parent < index => paths[identity.parent]
                    .clone()
                    .with_segment(EvidenceSegment::new(
                        identity.kind,
                        member_path(&identity.name),
                        identity.ordinal,
                    )),
                _ => return Err(invalid_tree()),
            };
            paths.push(path);
        }
        Ok(paths)
    }

    pub fn canonical_records(
        &self,
        root: RelativePath,
    ) -> Result<Vec<ExtractionRecord>, NativeError> {
        let paths = self.evidence_paths(root)?;
        let capabilities = parser_capabilities()?;
        let mut next_observation = 0_u64;
        let mut records = Vec::with_capacity(self.nodes.len());
        for (node, path) in self.nodes.iter().zip(paths) {
            let kind = match node.identity.as_ref().map(|identity| identity.kind) {
                None => SourceKind::File,
                Some(EvidenceKind::ArchiveMember) => SourceKind::ArchiveMember,
                Some(EvidenceKind::Attachment) => SourceKind::Attachment,
                Some(EvidenceKind::EmbeddedObject) => SourceKind::EmbeddedObject,
                Some(EvidenceKind::AlternateStream | EvidenceKind::Resource) => SourceKind::Stream,
            };
            let mut record = ExtractionRecord::new(SourceDescriptor {
                path: path.clone(),
                kind,
                identity: FileIdentity {
                    size_bytes: node.size_bytes,
                    modified_ns: None,
                    created_ns: None,
                    stable_id: node
                        .identity
                        .as_ref()
                        .and_then(|identity| identity.packet_id.clone()),
                },
                declared_media_type: None,
                detected_media_type: None,
            });
            record.status = ExtractionStatus::Unsupported;
            let mut complete = false;
            for result in &node.results {
                let capability = capabilities.iter().find(|cap| cap.id == result.parser_id);
                let parser = ParserDescriptor {
                    id: result.parser_id.clone(),
                    name: capability
                        .map_or_else(|| result.parser_id.clone(), |cap| cap.name.clone()),
                    version: capability.map_or_else(String::new, |cap| cap.version.clone()),
                    library: None,
                };
                for observation in &result.observations {
                    record.observations.push(observation.to_canonical(
                        ObservationId::new(next_observation),
                        path.clone(),
                        parser.clone(),
                    ));
                    next_observation = next_observation.checked_add(1).ok_or_else(invalid_tree)?;
                }
                if let Some(error) = &result.error {
                    if error.code != NativeErrorCode::UnsupportedParser {
                        record.diagnostics.push(Diagnostic {
                            code: format!("native.{:?}", error.code),
                            severity: DiagnosticSeverity::Warning,
                            stage: DiagnosticStage::Parse,
                            message: error.message.clone(),
                            parser: Some(parser),
                            source: path.clone(),
                            source_range: None,
                            related_observations: Vec::new(),
                            recoverable: error.recoverable,
                        });
                    }
                } else {
                    complete = true;
                }
            }
            if !record.diagnostics.is_empty() {
                record.status = if record.observations.is_empty() {
                    ExtractionStatus::Failed
                } else {
                    ExtractionStatus::Partial
                };
            } else if complete {
                record.status = ExtractionStatus::Complete;
            }
            let claims =
                independent_backend_claims(&record.observations, BackendClaimLimits::default());
            record.claims = claims.claims;
            if let Some(code) = claims.stopped {
                record.status = ExtractionStatus::Partial;
                record.diagnostics.push(Diagnostic {
                    code: format!("claims.{code:?}"),
                    severity: DiagnosticSeverity::Warning,
                    stage: DiagnosticStage::ResourceLimit,
                    message: "Independent claim generation reached its limit; original observations remain available".into(),
                    parser: None,
                    source: path,
                    source_range: None,
                    related_observations: Vec::new(),
                    recoverable: true,
                });
            }
            records.push(record);
        }
        if let (Some(code), Some(root)) = (self.stopped, records.first_mut()) {
            root.status = ExtractionStatus::Partial;
            root.diagnostics.push(Diagnostic {
                code: format!("recursive.{code:?}"),
                severity: DiagnosticSeverity::Warning,
                stage: DiagnosticStage::ResourceLimit,
                message: "Recursive extraction reached a shared resource limit; retained records are partial".into(),
                parser: None,
                source: root.source.path.clone(),
                source_range: None,
                related_observations: Vec::new(),
                recoverable: true,
            });
        }
        Ok(records)
    }
}
