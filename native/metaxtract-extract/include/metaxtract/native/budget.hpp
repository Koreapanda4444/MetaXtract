#pragma once

#include <chrono>
#include <cstdint>

namespace metaxtract::native {

struct ResourceLimits final {
    std::uint64_t max_source_bytes;
    std::uint64_t max_read_bytes;
    std::uint64_t max_allocation_bytes;
    std::uint64_t max_decoded_bytes;
    std::uint32_t max_container_depth;
    std::uint32_t max_entries;
    std::uint32_t max_observations;
    std::uint32_t max_wall_time_ms;
    std::uint64_t max_member_bytes = 64ULL * 1024 * 1024;
    std::uint32_t max_expansion_ratio = 1000;
};

enum class BudgetStatus : std::uint32_t {
    Ok = 0,
    SourceBytes = 10,
    ReadBytes = 11,
    AllocationBytes = 12,
    DecodedBytes = 13,
    ContainerDepth = 14,
    Entries = 15,
    Observations = 16,
    Deadline = 17,
    MemberBytes = 19,
    ExpansionRatio = 20,
};

class ParserBudget final {
public:
    ParserBudget(ResourceLimits limits, std::uint64_t source_size) noexcept;

    [[nodiscard]] bool consume_read(std::uint64_t amount) noexcept;
    [[nodiscard]] bool reserve_allocation(std::uint64_t amount) noexcept;
    [[nodiscard]] bool reserve_library_allocation(std::uint64_t amount) noexcept;
    [[nodiscard]] bool consume_decoded(std::uint64_t amount) noexcept;
    [[nodiscard]] bool check_member_size(std::uint64_t size) noexcept;
    [[nodiscard]] bool check_expansion(std::uint64_t decoded, std::uint64_t encoded) noexcept;
    [[nodiscard]] bool check_depth(std::uint32_t depth) noexcept;
    [[nodiscard]] bool add_entries(std::uint32_t amount) noexcept;
    [[nodiscard]] bool add_observations(std::uint32_t amount) noexcept;
    [[nodiscard]] bool checkpoint() noexcept;

    [[nodiscard]] BudgetStatus status() const noexcept;
    [[nodiscard]] std::uint64_t read_bytes() const noexcept;
    [[nodiscard]] std::uint64_t allocation_bytes() const noexcept;
    [[nodiscard]] std::uint64_t decoded_bytes() const noexcept;
    [[nodiscard]] std::uint32_t entries() const noexcept;
    [[nodiscard]] std::uint32_t observations() const noexcept;

private:
    [[nodiscard]] bool consume(
        std::uint64_t& counter,
        std::uint64_t amount,
        std::uint64_t maximum,
        BudgetStatus failure) noexcept;
    [[nodiscard]] bool consume(
        std::uint32_t& counter,
        std::uint32_t amount,
        std::uint32_t maximum,
        BudgetStatus failure) noexcept;
    bool fail(BudgetStatus status) noexcept;

    ResourceLimits limits_;
    std::uint64_t source_size_;
    BudgetStatus status_;
    std::chrono::steady_clock::time_point deadline_;
    std::uint64_t read_bytes_ = 0;
    std::uint64_t allocation_bytes_ = 0;
    std::uint64_t decoded_bytes_ = 0;
    std::uint32_t entries_ = 0;
    std::uint32_t observations_ = 0;
};

}
