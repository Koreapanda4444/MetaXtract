#include "metaxtract-native/src/ffi.rs.h"

#include "metaxtract/native/bridge.hpp"
#include "metaxtract/native/core.hpp"

namespace metaxtract::bridge {

FfiCoreInfo ffi_core_info() noexcept {
    const auto info = metaxtract::native::core_info();
    return FfiCoreInfo{
        .api_revision = info.api_revision,
        .schema_revision = info.schema_revision,
        .little_endian = info.little_endian,
    };
}

FfiStatus ffi_validate_revisions(
    const std::uint32_t expected_api,
    const std::uint32_t expected_schema) noexcept {
    const auto info = metaxtract::native::core_info();
    if (expected_api != info.api_revision || expected_schema != info.schema_revision) {
        return FfiStatus{
            .ok = false,
            .code = 1,
            .recoverable = false,
        };
    }
    return FfiStatus{
        .ok = true,
        .code = 0,
        .recoverable = false,
    };
}
}
