#include "metaxtract/native/parser.hpp"

#include <algorithm>

namespace metaxtract::native {

ParserContext::ParserContext(const RandomAccessInput& source, const ResourceLimits limits) noexcept
    : source_(source), budget_(limits, source.size()) {}

std::uint64_t ParserContext::source_size() const noexcept {
    return source_.size();
}

ReadChunk ParserContext::read_at(const std::uint64_t offset, const std::uint32_t requested) {
    if (offset > source_.size()) {
        return {.bytes = {}, .status = ReadStatus::OutOfRange};
    }
    const auto length = std::min<std::uint64_t>(requested, source_.size() - offset);
    if (!budget_.consume_read(length) || !budget_.reserve_allocation(2 * length)) {
        return {.bytes = {}, .status = ReadStatus::BudgetExhausted};
    }
    auto result = source_.read_at(offset, requested);
    if (result.status == ReadStatus::Ok && result.bytes.size() != length) {
        return {.bytes = {}, .status = ReadStatus::IoFailure};
    }
    return result;
}

ParserBudget& ParserContext::budget() noexcept {
    return budget_;
}

}
