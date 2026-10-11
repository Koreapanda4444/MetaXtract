#![deny(unsafe_code)]

#[allow(unsafe_code)]
mod ffi;

pub const NATIVE_API_REVISION: u32 = 1;
pub const NATIVE_SCHEMA_REVISION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeBuildInfo {
    pub api_revision: u32,
    pub schema_revision: u32,
    pub little_endian: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeErrorCode {
    RevisionMismatch,
    InvalidRequest,
    Internal,
    ResourceLimit,
    OutOfRange,
    Unknown(u32),
}

impl From<u32> for NativeErrorCode {
    fn from(value: u32) -> Self {
        match value {
            1 => Self::RevisionMismatch,
            2 => Self::InvalidRequest,
            3 => Self::Internal,
            4 => Self::ResourceLimit,
            5 => Self::OutOfRange,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputLimits {
    pub max_source_bytes: u64,
    pub max_single_read: u32,
    pub max_total_read: u64,
}

impl Default for InputLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 1024 * 1024 * 1024,
            max_single_read: 1024 * 1024,
            max_total_read: 8 * 1024 * 1024,
        }
    }
}

pub struct BoundedInput {
    source: ffi::ReadSource,
}

impl BoundedInput {
    pub fn from_bytes(bytes: Vec<u8>, limits: InputLimits) -> Result<Self, NativeError> {
        if limits.max_single_read == 0 || limits.max_total_read == 0 {
            return Err(native_error(NativeErrorCode::InvalidRequest, false));
        }
        if bytes.len() as u64 > limits.max_source_bytes {
            return Err(native_error(NativeErrorCode::ResourceLimit, true));
        }
        Ok(Self {
            source: ffi::ReadSource::new(
                bytes,
                limits.max_single_read,
                limits.max_total_read,
            ),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputCheck {
    pub source_size: u64,
    pub bytes_read: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParserCapability {
    pub id: String,
    pub name: String,
    pub version: String,
    pub operations: u32,
    pub formats: Vec<String>,
}

pub fn check_input(input: &BoundedInput, requested: u32) -> Result<InputCheck, NativeError> {
    let result = ffi::bridge::ffi_input_check(&input.source, requested);
    if result.status != 0 {
        return Err(read_error(result.status));
    }
    Ok(InputCheck {
        source_size: result.source_size,
        bytes_read: result.bytes_read,
    })
}

pub fn parser_capabilities() -> Vec<ParserCapability> {
    ffi::bridge::ffi_capabilities()
        .into_iter()
        .map(|capability| ParserCapability {
            id: capability.id,
            name: capability.name,
            version: capability.version,
            operations: capability.operations,
            formats: capability.formats,
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeError {
    pub code: NativeErrorCode,
    pub message: String,
    pub recoverable: bool,
}

pub fn build_info() -> Result<NativeBuildInfo, NativeError> {
    let status = ffi::bridge::ffi_validate_revisions(
        NATIVE_API_REVISION,
        NATIVE_SCHEMA_REVISION,
    );
    if !status.ok {
        let code = NativeErrorCode::from(status.code);
        return Err(NativeError {
            code,
            message: error_message(code).to_owned(),
            recoverable: status.recoverable,
        });
    }

    let info = ffi::bridge::ffi_core_info();
    Ok(NativeBuildInfo {
        api_revision: info.api_revision,
        schema_revision: info.schema_revision,
        little_endian: info.little_endian,
    })
}

fn error_message(code: NativeErrorCode) -> &'static str {
    match code {
        NativeErrorCode::RevisionMismatch => "native API or schema revision mismatch",
        NativeErrorCode::InvalidRequest => "native extraction request is invalid",
        NativeErrorCode::Internal => "native extraction failed internally",
        NativeErrorCode::ResourceLimit => "native extraction resource limit exceeded",
        NativeErrorCode::OutOfRange => "native extraction read is outside the source",
        NativeErrorCode::Unknown(_) => "native extraction returned an unknown error",
    }
}

fn native_error(code: NativeErrorCode, recoverable: bool) -> NativeError {
    NativeError {
        code,
        message: error_message(code).to_owned(),
        recoverable,
    }
}

fn read_error(status: u32) -> NativeError {
    match status {
        1 => native_error(NativeErrorCode::OutOfRange, true),
        2 => native_error(NativeErrorCode::InvalidRequest, true),
        3 => native_error(NativeErrorCode::ResourceLimit, true),
        4 => native_error(NativeErrorCode::Internal, false),
        other => native_error(NativeErrorCode::Unknown(other), false),
    }
}
