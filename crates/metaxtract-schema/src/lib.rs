#![forbid(unsafe_code)]

mod source;

pub use source::{
    ByteRange, ContainerDescriptor, EvidenceKind, EvidencePath, EvidenceSegment, FileIdentity,
    PathError, RelativePath, SourceDescriptor, SourceKind, StreamDescriptor, StreamKind,
};
