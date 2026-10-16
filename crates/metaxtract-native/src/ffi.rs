use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    sync::{Arc, Mutex},
};

#[derive(Clone)]
enum Source {
    Bytes(Arc<[u8]>),
    File(Arc<Mutex<File>>),
}

pub(crate) struct ReadSource {
    source: Source,
    size: u64,
    max_single_read: u32,
    max_total_read: u64,
    budget: Mutex<ReadBudget>,
}

struct ReadBudget {
    consumed: u64,
    maximum: u64,
}

impl ReadSource {
    pub(crate) fn new(bytes: Vec<u8>, max_single_read: u32, max_total_read: u64) -> Self {
        Self {
            size: bytes.len() as u64,
            source: Source::Bytes(bytes.into()),
            max_single_read,
            max_total_read,
            budget: Mutex::new(ReadBudget {
                consumed: 0,
                maximum: max_total_read,
            }),
        }
    }

    pub(crate) fn from_file(
        file: File,
        size: u64,
        max_single_read: u32,
        max_total_read: u64,
    ) -> Self {
        Self {
            source: Source::File(Arc::new(Mutex::new(file))),
            size,
            max_single_read,
            max_total_read,
            budget: Mutex::new(ReadBudget {
                consumed: 0,
                maximum: max_total_read,
            }),
        }
    }
}

impl ReadSource {
    pub(crate) fn parser_session(&self, maximum: u64) -> Self {
        let max_total_read = self.max_total_read.min(maximum);
        Self {
            source: self.source.clone(),
            size: self.size,
            max_single_read: self.max_single_read,
            max_total_read,
            budget: Mutex::new(ReadBudget {
                consumed: 0,
                maximum: max_total_read,
            }),
        }
    }
}

fn source_len(source: &ReadSource) -> u64 {
    source.size
}

fn source_read(source: &ReadSource, offset: u64, requested: u32) -> bridge::FfiReadResult {
    if requested > source.max_single_read {
        return bridge::FfiReadResult {
            data: Vec::new(),
            status: 2,
            source_size: source.size,
        };
    }

    if offset > source.size {
        return bridge::FfiReadResult {
            data: Vec::new(),
            status: 1,
            source_size: source.size,
        };
    }

    let length = (source.size - offset).min(u64::from(requested));
    let Ok(mut budget) = source.budget.lock() else {
        return bridge::FfiReadResult {
            data: Vec::new(),
            status: 4,
            source_size: source.size,
        };
    };
    let Some(next_consumed) = budget.consumed.checked_add(length) else {
        return bridge::FfiReadResult {
            data: Vec::new(),
            status: 3,
            source_size: source.size,
        };
    };
    if next_consumed > budget.maximum {
        return bridge::FfiReadResult {
            data: Vec::new(),
            status: 3,
            source_size: source.size,
        };
    }
    budget.consumed = next_consumed;

    let mut data = Vec::new();
    if data.try_reserve_exact(length as usize).is_err() {
        return bridge::FfiReadResult {
            data,
            status: 6,
            source_size: source.size,
        };
    }
    let status = match &source.source {
        Source::Bytes(bytes) => {
            let start = offset as usize;
            data.extend_from_slice(&bytes[start..start + length as usize]);
            0
        }
        Source::File(file) => {
            data.resize(length as usize, 0);
            match file.lock() {
                Ok(mut file) => {
                    if file.seek(SeekFrom::Start(offset)).is_ok()
                        && file.read_exact(&mut data).is_ok()
                    {
                        0
                    } else {
                        5
                    }
                }
                Err(_) => 4,
            }
        }
    };
    if status != 0 {
        data.clear();
    }
    bridge::FfiReadResult {
        data,
        status,
        source_size: source.size,
    }
}

#[cxx::bridge(namespace = "metaxtract::bridge")]
pub(crate) mod bridge {
    struct FfiCoreInfo {
        api_revision: u32,
        schema_revision: u32,
        little_endian: bool,
    }

    struct FfiStatus {
        ok: bool,
        code: u32,
        recoverable: bool,
    }

    struct FfiReadResult {
        data: Vec<u8>,
        status: u32,
        source_size: u64,
    }

    struct FfiInputCheck {
        source_size: u64,
        bytes_read: u32,
        status: u32,
    }

    struct FfiCapability {
        id: String,
        name: String,
        version: String,
        operations: u32,
        formats: Vec<String>,
    }

    struct FfiCapabilities {
        values: Vec<FfiCapability>,
        status: u32,
    }

    struct FfiParserRun {
        status: u32,
        recoverable: bool,
        read_bytes: u64,
        allocation_bytes: u64,
        decoded_bytes: u64,
        entries: u32,
        observations: u32,
    }

    struct FfiContentCandidate {
        format: String,
        mime: String,
        container: String,
        state: u32,
        offset: u64,
        length: u64,
        detail: String,
    }

    struct FfiDetection {
        candidates: Vec<FfiContentCandidate>,
        status: u32,
        recoverable: bool,
    }

    struct FfiObservation {
        name_space: String,
        key: String,
        kind: u32,
        value: Vec<u8>,
        encoded: Vec<u8>,
        encoding: String,
        offset: u64,
        length: u64,
        located: bool,
        tag_code: u64,
        numeric_tag: bool,
        method: u32,
        stream_id: String,
    }

    struct FfiExtraction {
        run: FfiParserRun,
        values: Vec<FfiObservation>,
    }

    struct FfiResourceLimits {
        max_source_bytes: u64,
        max_read_bytes: u64,
        max_allocation_bytes: u64,
        max_decoded_bytes: u64,
        max_container_depth: u32,
        max_entries: u32,
        max_observations: u32,
        max_wall_time_ms: u32,
        max_member_bytes: u64,
        max_expansion_ratio: u32,
    }

    struct FfiBudgetStatus {
        status: u32,
    }

    extern "Rust" {
        type ReadSource;

        fn source_len(source: &ReadSource) -> u64;
        fn source_read(source: &ReadSource, offset: u64, requested: u32) -> FfiReadResult;
    }

    unsafe extern "C++" {
        include!("metaxtract/native/bridge.hpp");

        fn ffi_core_info() -> FfiCoreInfo;
        fn ffi_validate_revisions(expected_api: u32, expected_schema: u32) -> FfiStatus;
        fn ffi_input_check(source: &ReadSource, requested: u32) -> FfiInputCheck;
        fn ffi_input_read(source: &ReadSource, offset: u64, requested: u32) -> FfiReadResult;
        fn ffi_capabilities() -> FfiCapabilities;
        fn ffi_worker_memory_limit(bytes: u64) -> bool;
        fn ffi_worker_memory_usage(pid: u32) -> u64;
        fn ffi_detect(source: &ReadSource, limits: FfiResourceLimits) -> FfiDetection;
        fn ffi_extract(
            source: &ReadSource,
            parser_id: &str,
            limits: FfiResourceLimits,
        ) -> FfiExtraction;
        fn ffi_run_parser(
            source: &ReadSource,
            parser_id: &str,
            limits: FfiResourceLimits,
        ) -> FfiParserRun;
        fn ffi_validate_limits(source_size: u64, limits: FfiResourceLimits) -> FfiBudgetStatus;
    }
}
