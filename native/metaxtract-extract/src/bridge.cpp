#include "metaxtract-native/src/ffi.rs.h"

#include "metaxtract/native/bridge.hpp"
#include "metaxtract/native/budget.hpp"
#include "metaxtract/native/core.hpp"
#include "metaxtract/native/input.hpp"
#include "metaxtract/native/registry.hpp"

#include <algorithm>
#include <utility>
#include <vector>

namespace metaxtract::bridge {

namespace {

class RustInput final : public metaxtract::native::RandomAccessInput {
public:
    explicit RustInput(const ReadSource& source) noexcept : source_(source) {}

    std::uint64_t size() const noexcept override {
        return source_len(source_);
    }

    metaxtract::native::ReadChunk read_at(
        const std::uint64_t offset,
        const std::uint32_t requested) const noexcept override {
        try {
            auto result = source_read(source_, offset, requested);
            return metaxtract::native::ReadChunk{
                .bytes = std::vector<std::uint8_t>(result.data.begin(), result.data.end()),
                .status = static_cast<metaxtract::native::ReadStatus>(result.status),
            };
        } catch (...) {
            return metaxtract::native::ReadChunk{
                .bytes = {},
                .status = metaxtract::native::ReadStatus::Internal,
            };
        }
    }

private:
    const ReadSource& source_;
};

}

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

FfiInputCheck ffi_input_check(
    const ReadSource& source,
    const std::uint32_t requested) noexcept {
    const RustInput input(source);
    const auto chunk = input.read_at(0, requested);
    return FfiInputCheck{
        .source_size = input.size(),
        .bytes_read = static_cast<std::uint32_t>(chunk.bytes.size()),
        .status = static_cast<std::uint32_t>(chunk.status),
    };
}

rust::Vec<FfiCapability> ffi_capabilities() noexcept {
    rust::Vec<FfiCapability> result;
    try {
        for (const auto& parser : metaxtract::native::default_registry().parsers()) {
            const auto& capability = parser->capability();
            rust::Vec<rust::String> formats;
            for (const auto& format : capability.formats) {
                formats.push_back(rust::String(format));
            }
            result.push_back(FfiCapability{
                .id = rust::String(capability.id),
                .name = rust::String(capability.name),
                .version = rust::String(capability.version),
                .operations = capability.operations,
                .formats = std::move(formats),
            });
        }
    } catch (...) {
        return {};
    }
    return result;
}

FfiBudgetStatus ffi_validate_limits(
    const std::uint64_t source_size,
    const FfiResourceLimits limits) noexcept {
    const metaxtract::native::ResourceLimits native_limits{
        .max_source_bytes = limits.max_source_bytes,
        .max_read_bytes = limits.max_read_bytes,
        .max_allocation_bytes = limits.max_allocation_bytes,
        .max_decoded_bytes = limits.max_decoded_bytes,
        .max_container_depth = limits.max_container_depth,
        .max_entries = limits.max_entries,
        .max_observations = limits.max_observations,
        .max_wall_time_ms = limits.max_wall_time_ms,
    };
    metaxtract::native::ParserBudget budget(native_limits, source_size);
    static_cast<void>(budget.checkpoint());
    return FfiBudgetStatus{
        .status = static_cast<std::uint32_t>(budget.status()),
    };
}
}
