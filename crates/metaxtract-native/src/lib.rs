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
    Unknown(u32),
}

impl From<u32> for NativeErrorCode {
    fn from(value: u32) -> Self {
        match value {
            1 => Self::RevisionMismatch,
            2 => Self::InvalidRequest,
            3 => Self::Internal,
            other => Self::Unknown(other),
        }
    }
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
        NativeErrorCode::Unknown(_) => "native extraction returned an unknown error",
    }
}
