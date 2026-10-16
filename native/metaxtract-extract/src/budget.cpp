#include "metaxtract/native/budget.hpp"

#include <utility>

namespace metaxtract::native {

ParserBudget::ParserBudget(ResourceLimits limits, const std::uint64_t source_size) noexcept
    : limits_(limits),
      source_size_(source_size),
      status_(source_size <= limits.max_source_bytes ? BudgetStatus::Ok : BudgetStatus::SourceBytes),
      deadline_(
          std::chrono::steady_clock::now()
          + std::chrono::milliseconds(limits.max_wall_time_ms)) {}

bool ParserBudget::consume_read(const std::uint64_t amount) noexcept {
    return consume(read_bytes_, amount, limits_.max_read_bytes, BudgetStatus::ReadBytes);
}

bool ParserBudget::reserve_allocation(const std::uint64_t amount) noexcept {
    return consume(
        allocation_bytes_,
        amount,
        limits_.max_allocation_bytes,
        BudgetStatus::AllocationBytes);
}

bool ParserBudget::reserve_library_allocation(const std::uint64_t amount) noexcept {
    // C libraries may allocate while reporting an earlier read/decode failure.
    // Keep accounting under the same ceiling without replacing the first error.
    static_cast<void>(checkpoint());
    if (amount > limits_.max_allocation_bytes - allocation_bytes_) {
        if (status_ == BudgetStatus::Ok) { static_cast<void>(fail(BudgetStatus::AllocationBytes)); }
        return false;
    }
    allocation_bytes_ += amount;
    return true;
}

bool ParserBudget::consume_decoded(const std::uint64_t amount) noexcept {
    if (!checkpoint()) { return false; }
    if (amount > limits_.max_decoded_bytes - decoded_bytes_) { return fail(BudgetStatus::DecodedBytes); }
    if (!check_expansion(decoded_bytes_ + amount, source_size_)) { return false; }
    return consume(decoded_bytes_, amount, limits_.max_decoded_bytes, BudgetStatus::DecodedBytes);
}

bool ParserBudget::check_member_size(const std::uint64_t size) noexcept {
    return checkpoint() && (size <= limits_.max_member_bytes || fail(BudgetStatus::MemberBytes));
}

bool ParserBudget::check_expansion(const std::uint64_t decoded, const std::uint64_t encoded) noexcept {
    if (!checkpoint()) { return false; }
    const auto ratio = limits_.max_expansion_ratio;
    if (ratio == 0 || encoded > UINT64_MAX / ratio) { return true; }
    return decoded <= encoded * ratio || fail(BudgetStatus::ExpansionRatio);
}

bool ParserBudget::check_depth(const std::uint32_t depth) noexcept {
    if (!checkpoint()) {
        return false;
    }
    return depth <= limits_.max_container_depth || fail(BudgetStatus::ContainerDepth);
}

bool ParserBudget::add_entries(const std::uint32_t amount) noexcept {
    return consume(entries_, amount, limits_.max_entries, BudgetStatus::Entries);
}

bool ParserBudget::add_observations(const std::uint32_t amount) noexcept {
    return consume(
        observations_,
        amount,
        limits_.max_observations,
        BudgetStatus::Observations);
}

bool ParserBudget::checkpoint() noexcept {
    if (status_ != BudgetStatus::Ok) {
        return false;
    }
    if (std::chrono::steady_clock::now() >= deadline_) {
        return fail(BudgetStatus::Deadline);
    }
    return true;
}

BudgetStatus ParserBudget::status() const noexcept {
    return status_;
}

std::uint64_t ParserBudget::read_bytes() const noexcept {
    return read_bytes_;
}

std::uint64_t ParserBudget::allocation_bytes() const noexcept {
    return allocation_bytes_;
}

std::uint64_t ParserBudget::decoded_bytes() const noexcept {
    return decoded_bytes_;
}

std::uint32_t ParserBudget::entries() const noexcept {
    return entries_;
}

std::uint32_t ParserBudget::observations() const noexcept {
    return observations_;
}

bool ParserBudget::consume(
    std::uint64_t& counter,
    const std::uint64_t amount,
    const std::uint64_t maximum,
    const BudgetStatus failure) noexcept {
    if (!checkpoint()) {
        return false;
    }
    if (amount > maximum - counter) {
        return fail(failure);
    }
    counter += amount;
    return true;
}

bool ParserBudget::consume(
    std::uint32_t& counter,
    const std::uint32_t amount,
    const std::uint32_t maximum,
    const BudgetStatus failure) noexcept {
    if (!checkpoint()) {
        return false;
    }
    if (amount > maximum - counter) {
        return fail(failure);
    }
    counter += amount;
    return true;
}

bool ParserBudget::fail(const BudgetStatus status) noexcept {
    status_ = status;
    return false;
}

}
