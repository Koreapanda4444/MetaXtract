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

    unsafe extern "C++" {
        include!("metaxtract/native/bridge.hpp");

        fn ffi_core_info() -> FfiCoreInfo;
        fn ffi_validate_revisions(expected_api: u32, expected_schema: u32) -> FfiStatus;
    }
}
