#![forbid(unsafe_code)]

mod diagnostic;
mod provenance;
mod raw;
mod source;

pub use diagnostic::{Diagnostic, DiagnosticSeverity, DiagnosticStage};
pub use provenance::{ExtractionMethod, LibraryDescriptor, ParserDescriptor, Provenance};
pub use raw::{
    MetadataObservation, ObservationId, Rational, RawField, RawValue, TagCode, TagIdentity,
    TextValue,
};
pub use source::{
    ByteRange, ContainerDescriptor, EvidenceKind, EvidencePath, EvidenceSegment, FileIdentity,
    PathError, RelativePath, SourceDescriptor, SourceKind, StreamDescriptor, StreamKind,
};
