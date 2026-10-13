mod walk;
pub use walk::{
    FileStamp, NativePath, OpenedEvidence, ScanRoot, SymlinkPolicy, WalkEntry, WalkOptions,
};

mod hash;
pub use hash::{EvidenceHashes, HashOptions, hash_evidence};

mod pipeline;
pub use pipeline::{
    ExtractionBackend, ExtractionBatch, FileTask, NativeExtractor, ScanEvent, ScanOptions,
    ScanSummary, ScannedFile, scan_stream,
};
