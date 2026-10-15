#![forbid(unsafe_code)]

pub mod preview;
pub mod scan;
pub mod workflows;
pub use metaxtract_analysis as analysis;
pub use metaxtract_core as core;
pub use metaxtract_schema as schema;
pub use metaxtract_storage as storage;
pub use zeroize::Zeroizing;

use std::{io, path::Path};
use storage::{AccessMode, CaseWorkspace, SearchQuery};

pub fn open_case(path: &Path, writable: bool) -> io::Result<CaseWorkspace> {
    CaseWorkspace::open(
        path,
        if writable {
            AccessMode::ReadWrite
        } else {
            AccessMode::ReadOnly
        },
    )
}
pub fn collect(
    case: &CaseWorkspace,
    query: &SearchQuery,
) -> io::Result<Vec<schema::ExtractionRecord>> {
    let limits = analysis::AnalysisLimits::default();
    let info = case.info()?;
    let mut records = Vec::new();
    let mut cursor = None;
    let mut bytes = 0usize;
    loop {
        let page = case.search_records(query, cursor.as_ref(), 128)?;
        for header in page.items {
            let record = case
                .record(header.id)?
                .ok_or_else(|| io::Error::other("record disappeared"))?;
            bytes =
                bytes.saturating_add(serde_json::to_vec(&record).map_err(io::Error::other)?.len());
            if records.len() >= limits.max_records || bytes > limits.max_bytes {
                return Err(io::Error::other(
                    "selection exceeds analysis budget; narrow the search",
                ));
            }
            records.push(record);
        }
        if page.next.is_none() {
            break;
        }
        cursor = page.next;
    }
    if case.info()?.revision != info.revision {
        return Err(io::Error::other("case changed during analysis"));
    }
    Ok(records)
}
pub fn read_bytes(path: &Path, max: usize) -> io::Result<Vec<u8>> {
    use io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "input exceeds byte budget",
        ));
    }
    Ok(bytes)
}
