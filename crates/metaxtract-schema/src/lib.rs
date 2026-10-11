#![forbid(unsafe_code)]

mod claim;
mod diagnostic;
mod provenance;
mod raw;
mod record;
mod source;

pub use claim::{
    AgreementLevel, CanonicalKey, ClaimConflict, ClaimId, ClaimResolution, Confidence,
    ConfidenceBasis, GeoPoint, KeyError, NormalizedClaim, NormalizedValue, TimestampPrecision,
    TimestampValue,
};
pub use diagnostic::{Diagnostic, DiagnosticSeverity, DiagnosticStage};
pub use provenance::{ExtractionMethod, LibraryDescriptor, ParserDescriptor, Provenance};
pub use raw::{
    MetadataObservation, ObservationId, Rational, RawField, RawValue, TagCode, TagIdentity,
    TextValue,
};
pub use record::{ContentDigest, DigestAlgorithm, ExtractionRecord, ExtractionStatus};
pub use source::{
    ByteRange, ContainerDescriptor, EvidenceKind, EvidencePath, EvidenceSegment, FileIdentity,
    PathError, RelativePath, SourceDescriptor, SourceKind, StreamDescriptor, StreamKind,
};
