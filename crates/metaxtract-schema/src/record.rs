use crate::{
    ClaimConflict, ContainerDescriptor, Diagnostic, MetadataObservation, NormalizedClaim,
    SourceDescriptor,
};

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum DigestAlgorithm {
    Sha256,
    Blake3,
    Other(String),
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct ContentDigest {
    pub algorithm: DigestAlgorithm,
    pub bytes: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExtractionStatus {
    Complete,
    Partial,
    Unsupported,
    Failed,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct ExtractionRecord {
    pub source: SourceDescriptor,
    pub digests: Vec<ContentDigest>,
    pub containers: Vec<ContainerDescriptor>,
    pub observations: Vec<MetadataObservation>,
    pub claims: Vec<NormalizedClaim>,
    pub conflicts: Vec<ClaimConflict>,
    pub diagnostics: Vec<Diagnostic>,
    pub status: ExtractionStatus,
}

impl ExtractionRecord {
    pub fn new(source: SourceDescriptor) -> Self {
        Self {
            source,
            digests: Vec::new(),
            containers: Vec::new(),
            observations: Vec::new(),
            claims: Vec::new(),
            conflicts: Vec::new(),
            diagnostics: Vec::new(),
            status: ExtractionStatus::Complete,
        }
    }
}
