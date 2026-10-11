use crate::{ByteRange, EvidencePath};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibraryDescriptor {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParserDescriptor {
    pub id: String,
    pub name: String,
    pub version: String,
    pub library: Option<LibraryDescriptor>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExtractionMethod {
    Structure,
    Direct,
    NativeLibrary,
    DecodedPayload,
    Heuristic,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Provenance {
    pub parser: ParserDescriptor,
    pub source: EvidencePath,
    pub stream_id: Option<String>,
    pub source_range: Option<ByteRange>,
    pub method: ExtractionMethod,
    pub sequence: u64,
}
