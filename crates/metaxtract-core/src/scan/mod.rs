//! Bounded, read-only evidence discovery and processing.
mod cache;
mod storage;
pub use storage::{ScanCheckpoint, ScanStore, TaskRow, TaskState};
mod codec;
mod control;
mod hash;
mod memory;
pub use cache::{CachedExtractor, RecordCache, engine_fingerprint};
mod pipeline;
mod walk;
pub use control::{ExecutionContext, ScanControl, StopReason};
pub use hash::{EvidenceHashes, HashOptions, hash_evidence};
pub use pipeline::{
    ExtractionBackend, ExtractionBatch, FileTask, NativeExtractor, ScanEvent, ScanOptions,
    ScanSummary, ScannedFile, scan_stream, scan_stream_with_control,
};
pub use walk::{
    FileStamp, NativePath, OpenedEvidence, ScanRoot, SymlinkPolicy, WalkEntry, WalkOptions,
};
