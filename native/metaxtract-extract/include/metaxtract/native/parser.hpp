#pragma once

#include "metaxtract/native/input.hpp"
#include "metaxtract/native/budget.hpp"

#include <cstdint>
#include <string>
#include <vector>

namespace metaxtract::native {

enum ParserOperation : std::uint32_t {
    Probe = 1U << 0U,
    Extract = 1U << 1U,
    DecodePreview = 1U << 2U,
    EnumerateChildren = 1U << 3U,
};

struct ParserCapability final {
    std::string id;
    std::string name;
    std::string version;
    std::uint32_t operations;
    std::vector<std::string> formats;
};

class ParserContext final {
public:
    ParserContext(const RandomAccessInput& source, ResourceLimits limits) noexcept;

    [[nodiscard]] std::uint64_t source_size() const noexcept;
    [[nodiscard]] ReadChunk read_at(std::uint64_t offset, std::uint32_t requested);
    [[nodiscard]] ParserBudget& budget() noexcept;

private:
    const RandomAccessInput& source_;
    ParserBudget budget_;
};

enum class ParseStatus : std::uint32_t {
    Complete = 0,
    InvalidRequest = 2,
    Internal = 3,
    OutOfRange = 5,
    ReadFailure = 6,
    Unsupported = 7,
    AllocationFailure = 8,
};

struct ParserRun final {
    std::uint32_t status = 0;
    bool recoverable = false;
    std::uint64_t read_bytes = 0;
    std::uint64_t allocation_bytes = 0;
    std::uint64_t decoded_bytes = 0;
    std::uint32_t entries = 0;
    std::uint32_t observations = 0;
};

class Parser {
public:
    virtual ~Parser() = default;

    [[nodiscard]] virtual const ParserCapability& capability() const noexcept = 0;
    [[nodiscard]] virtual ParseStatus run(ParserContext& context) const = 0;
};

}
