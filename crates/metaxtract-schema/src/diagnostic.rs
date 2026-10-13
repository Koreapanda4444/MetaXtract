use crate::{ByteRange, EvidencePath, ObservationId, ParserDescriptor};

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DiagnosticSeverity {
    Information,
    Warning,
    Error,
    Fatal,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DiagnosticStage {
    Probe,
    Structure,
    Parse,
    Decode,
    ResourceLimit,
    Integrity,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub code: String,
    pub severity: DiagnosticSeverity,
    pub stage: DiagnosticStage,
    pub message: String,
    pub parser: Option<ParserDescriptor>,
    pub source: EvidencePath,
    pub source_range: Option<ByteRange>,
    pub related_observations: Vec<ObservationId>,
    pub recoverable: bool,
}
