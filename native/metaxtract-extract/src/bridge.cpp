#include "metaxtract-native/src/ffi.rs.h"

#include "metaxtract/native/bridge.hpp"
#include "metaxtract/native/budget.hpp"
#include "metaxtract/native/core.hpp"
#include "metaxtract/native/detection.hpp"
#include "metaxtract/native/input.hpp"
#include "metaxtract/native/registry.hpp"

#include <algorithm>
#include <new>
#include <stdexcept>
#include <utility>
#include <vector>

namespace metaxtract::bridge {

namespace {

rust::String observation_identity(const std::string& value) {
    // String identities may contain arbitrary tag/stream bytes. Reserve this
    // prefix and encode invalid UTF-8 losslessly instead of dropping a run.
    if (!value.starts_with("~raw-bytes:")) {
        try { return rust::String(value); }
        catch (const std::invalid_argument&) {}
    }
    constexpr char hex[] = "0123456789abcdef";
    std::string encoded = "~raw-bytes:";
    encoded.reserve(encoded.size() + value.size() * 2);
    for (const auto character : value) {
        const auto byte = static_cast<unsigned char>(character);
        encoded.push_back(hex[byte >> 4U]); encoded.push_back(hex[byte & 15U]);
    }
    return rust::String(encoded);
}

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

metaxtract::native::ResourceLimits convert_limits(const FfiResourceLimits limits) noexcept {
    return {
        .max_source_bytes = limits.max_source_bytes,
        .max_read_bytes = limits.max_read_bytes,
        .max_allocation_bytes = limits.max_allocation_bytes,
        .max_decoded_bytes = limits.max_decoded_bytes,
        .max_container_depth = limits.max_container_depth,
        .max_entries = limits.max_entries,
        .max_observations = limits.max_observations,
        .max_wall_time_ms = limits.max_wall_time_ms,
    };
}

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

FfiReadResult ffi_input_read(
    const ReadSource& source,
    const std::uint64_t offset,
    const std::uint32_t requested) noexcept {
    try {
        return source_read(source, offset, requested);
    } catch (...) {
        return {.data = {}, .status = 4, .source_size = source_len(source)};
    }
}

FfiCapabilities ffi_capabilities() noexcept {
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
        return {.values = {}, .status = 3};
    }
    return {.values = std::move(result), .status = 0};
}

FfiParserRun ffi_run_parser(
    const ReadSource& source,
    const rust::Str parser_id,
    const FfiResourceLimits limits) noexcept {
    const RustInput input(source);
    const auto run = metaxtract::native::run_parser(
        metaxtract::native::default_registry(),
        std::string_view(parser_id.data(), parser_id.size()),
        input,
        convert_limits(limits));
    return {
        .status = run.status,
        .recoverable = run.recoverable,
        .read_bytes = run.read_bytes,
        .allocation_bytes = run.allocation_bytes,
        .decoded_bytes = run.decoded_bytes,
        .entries = run.entries,
        .observations = run.observations,
    };
}

FfiDetection ffi_detect(const ReadSource& source, const FfiResourceLimits limits) noexcept {
    try {
        const RustInput input(source);
        metaxtract::native::ParserContext context(input, convert_limits(limits));
        if (limits.max_wall_time_ms == 0) { return {.candidates = {}, .status = 2, .recoverable = false}; }
        rust::Vec<FfiContentCandidate> candidates;
        if (context.budget().checkpoint()) {
            for (const auto& value : metaxtract::native::detect_content(context)) {
                candidates.push_back({
                    .format = rust::String(value.format),
                    .mime = rust::String(value.mime),
                    .container = rust::String(value.container),
                    .state = static_cast<std::uint32_t>(value.state),
                    .offset = value.offset,
                    .length = value.length,
                    .detail = rust::String(value.detail),
                });
            }
        }
        static_cast<void>(context.budget().checkpoint());
        const auto status = static_cast<std::uint32_t>(context.budget().status());
        return {.candidates = std::move(candidates), .status = status, .recoverable = status != 0};
    } catch (const metaxtract::native::DetectionFailure& failure) {
        return {.candidates = {}, .status = failure.status, .recoverable = failure.status != 2 && failure.status != 3 && failure.status != 8};
    } catch (const std::bad_alloc&) {
        return {.candidates = {}, .status = 8, .recoverable = false};
    } catch (...) {
        return {.candidates = {}, .status = 3, .recoverable = false};
    }
}

FfiExtraction ffi_extract(const ReadSource& source, const rust::Str parser_id, const FfiResourceLimits limits) noexcept {
    try {
        const RustInput input(source);
        auto run = metaxtract::native::run_parser(metaxtract::native::default_registry(),
            std::string_view(parser_id.data(), parser_id.size()), input, convert_limits(limits));
        rust::Vec<FfiObservation> values;
        for (auto& value : run.values) {
            rust::Vec<std::uint8_t> encoded;
            for (const auto byte : value.encoded) { encoded.push_back(byte); }
            rust::Vec<std::uint8_t> text;
            for (const auto byte : value.value) { text.push_back(static_cast<std::uint8_t>(byte)); }
            values.push_back({
                .name_space = observation_identity(value.name_space), .key = observation_identity(value.key),
                .kind = static_cast<std::uint32_t>(value.kind), .value = std::move(text),
                .encoded = std::move(encoded), .encoding = observation_identity(value.encoding),
                .offset = value.offset, .length = value.length, .located = value.located,
                .tag_code = value.tag_code, .numeric_tag = value.numeric_tag,
                .method = value.method, .stream_id = observation_identity(value.stream_id),
            });
        }
        return {.run = {
            .status = run.status, .recoverable = run.recoverable,
            .read_bytes = run.read_bytes, .allocation_bytes = run.allocation_bytes,
            .decoded_bytes = run.decoded_bytes, .entries = run.entries, .observations = run.observations,
        }, .values = std::move(values)};
    } catch (const std::bad_alloc&) {
        return {.run = {.status = 8, .recoverable = false, .read_bytes = 0, .allocation_bytes = 0,
            .decoded_bytes = 0, .entries = 0, .observations = 0}, .values = {}};
    } catch (...) {
        return {.run = {.status = 3, .recoverable = false, .read_bytes = 0, .allocation_bytes = 0,
            .decoded_bytes = 0, .entries = 0, .observations = 0}, .values = {}};
    }
}

FfiBudgetStatus ffi_validate_limits(
    const std::uint64_t source_size,
    const FfiResourceLimits limits) noexcept {
    const auto native_limits = convert_limits(limits);
    metaxtract::native::ParserBudget budget(native_limits, source_size);
    static_cast<void>(budget.checkpoint());
    return FfiBudgetStatus{
        .status = static_cast<std::uint32_t>(budget.status()),
    };
}
}
