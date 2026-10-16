#pragma once

#include <cstdint>

#include "rust/cxx.h"

namespace metaxtract::bridge {

struct FfiCoreInfo;
struct FfiStatus;
struct FfiInputCheck;
struct FfiCapability;
struct FfiCapabilities;
struct FfiParserRun;
struct FfiReadResult;
struct FfiResourceLimits;
struct FfiBudgetStatus;
struct FfiDetection;
struct FfiExtraction;
struct ReadSource;

[[nodiscard]] FfiCoreInfo ffi_core_info() noexcept;
[[nodiscard]] FfiStatus ffi_validate_revisions(
    std::uint32_t expected_api,
    std::uint32_t expected_schema) noexcept;
[[nodiscard]] FfiInputCheck ffi_input_check(
    const ReadSource& source,
    std::uint32_t requested) noexcept;
[[nodiscard]] FfiReadResult ffi_input_read(
    const ReadSource& source,
    std::uint64_t offset,
    std::uint32_t requested) noexcept;
[[nodiscard]] FfiCapabilities ffi_capabilities() noexcept;
[[nodiscard]] bool ffi_worker_memory_limit(std::uint64_t bytes) noexcept;
[[nodiscard]] std::uint64_t ffi_worker_memory_usage(std::uint32_t pid) noexcept;
[[nodiscard]] FfiDetection ffi_detect(const ReadSource& source, FfiResourceLimits limits) noexcept;
[[nodiscard]] FfiExtraction ffi_extract(const ReadSource& source, rust::Str parser_id, FfiResourceLimits limits) noexcept;
[[nodiscard]] FfiParserRun ffi_run_parser(
    const ReadSource& source,
    rust::Str parser_id,
    FfiResourceLimits limits) noexcept;
[[nodiscard]] FfiBudgetStatus ffi_validate_limits(
    std::uint64_t source_size,
    FfiResourceLimits limits) noexcept;

}
