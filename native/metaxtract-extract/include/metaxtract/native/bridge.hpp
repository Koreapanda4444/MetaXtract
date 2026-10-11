#pragma once

#include <cstdint>

#include "rust/cxx.h"

namespace metaxtract::bridge {

struct FfiCoreInfo;
struct FfiStatus;
struct FfiInputCheck;
struct FfiCapability;
struct ReadSource;

[[nodiscard]] FfiCoreInfo ffi_core_info() noexcept;
[[nodiscard]] FfiStatus ffi_validate_revisions(
    std::uint32_t expected_api,
    std::uint32_t expected_schema) noexcept;
[[nodiscard]] FfiInputCheck ffi_input_check(
    const ReadSource& source,
    std::uint32_t requested) noexcept;
[[nodiscard]] rust::Vec<FfiCapability> ffi_capabilities() noexcept;

}
