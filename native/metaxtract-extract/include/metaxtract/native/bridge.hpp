#pragma once

#include <cstdint>

#include "rust/cxx.h"

namespace metaxtract::bridge {

struct FfiCoreInfo;
struct FfiStatus;
struct FfiInputCheck;
struct FfiCapability;
struct FfiResourceLimits;
struct FfiBudgetStatus;
struct ReadSource;

[[nodiscard]] FfiCoreInfo ffi_core_info() noexcept;
[[nodiscard]] FfiStatus ffi_validate_revisions(
    std::uint32_t expected_api,
    std::uint32_t expected_schema) noexcept;
[[nodiscard]] FfiInputCheck ffi_input_check(
    const ReadSource& source,
    std::uint32_t requested) noexcept;
[[nodiscard]] rust::Vec<FfiCapability> ffi_capabilities() noexcept;
[[nodiscard]] FfiBudgetStatus ffi_validate_limits(
    std::uint64_t source_size,
    FfiResourceLimits limits) noexcept;

}
