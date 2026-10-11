use std::sync::{Arc, Mutex};

pub(crate) struct ReadSource {
    bytes: Arc<[u8]>,
    max_single_read: u32,
    budget: Mutex<ReadBudget>,
}

struct ReadBudget {
    consumed: u64,
    maximum: u64,
}

impl ReadSource {
    pub(crate) fn new(bytes: Vec<u8>, max_single_read: u32, max_total_read: u64) -> Self {
        Self {
            bytes: bytes.into(),
            max_single_read,
            budget: Mutex::new(ReadBudget {
                consumed: 0,
                maximum: max_total_read,
            }),
        }
    }
}

fn source_len(source: &ReadSource) -> u64 {
    source.bytes.len() as u64
}

fn source_read(source: &ReadSource, offset: u64, requested: u32) -> bridge::FfiReadResult {
    if requested > source.max_single_read {
        return bridge::FfiReadResult {
            data: Vec::new(),
            status: 2,
        };
    }

    let Ok(start) = usize::try_from(offset) else {
        return bridge::FfiReadResult {
            data: Vec::new(),
            status: 1,
        };
    };
    if start > source.bytes.len() {
        return bridge::FfiReadResult {
            data: Vec::new(),
            status: 1,
        };
    }

    let available = source.bytes.len() - start;
    let length = available.min(requested as usize);
    let Ok(mut budget) = source.budget.lock() else {
        return bridge::FfiReadResult {
            data: Vec::new(),
            status: 4,
        };
    };
    let Some(next_consumed) = budget.consumed.checked_add(length as u64) else {
        return bridge::FfiReadResult {
            data: Vec::new(),
            status: 3,
        };
    };
    if next_consumed > budget.maximum {
        return bridge::FfiReadResult {
            data: Vec::new(),
            status: 3,
        };
    }
    budget.consumed = next_consumed;

    bridge::FfiReadResult {
        data: source.bytes[start..start + length].to_vec(),
        status: 0,
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
        fn ffi_capabilities() -> Vec<FfiCapability>;
    }
}
