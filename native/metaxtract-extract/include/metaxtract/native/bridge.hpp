#pragma once

#include <cstdint>

namespace metaxtract::bridge {

struct FfiCoreInfo;
struct FfiStatus;

[[nodiscard]] FfiCoreInfo ffi_core_info() noexcept;
[[nodiscard]] FfiStatus ffi_validate_revisions(
    std::uint32_t expected_api,
    std::uint32_t expected_schema) noexcept;

}
