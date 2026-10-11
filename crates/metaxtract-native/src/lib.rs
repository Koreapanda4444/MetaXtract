#![forbid(unsafe_code)]

pub const NATIVE_API_REVISION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeBuildInfo {
    pub api_revision: u32,
    pub schema_revision: u32,
    pub little_endian: bool,
}
