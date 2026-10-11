#pragma once

#include <cstddef>
#include <cstdint>
#include <vector>

namespace metaxtract::native {

enum class ReadStatus : std::uint32_t {
    Ok = 0,
    OutOfRange = 1,
    RequestTooLarge = 2,
    BudgetExhausted = 3,
    Internal = 4,
};

struct ReadChunk final {
    std::vector<std::uint8_t> bytes;
    ReadStatus status;
};

class RandomAccessInput {
public:
    virtual ~RandomAccessInput() = default;

    [[nodiscard]] virtual std::uint64_t size() const noexcept = 0;
    [[nodiscard]] virtual ReadChunk read_at(
        std::uint64_t offset,
        std::uint32_t requested) const noexcept = 0;
};

}
