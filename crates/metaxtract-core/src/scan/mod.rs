//! Bounded, read-only evidence discovery and processing.
mod cache;
mod incremental;
mod monitor;
pub use monitor::{ChangeBatch, DirectoryChange, DirectoryMonitor, MonitorOptions};
pub use incremental::{IncrementalSummary, InventoryEntry, ScanInventory};
mod checkpoint;
mod credentials;
mod worker;
pub use credentials::PdfPassword;
pub use worker::{BundledWorker, run_bundled_worker_stdio, scan_local};
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
