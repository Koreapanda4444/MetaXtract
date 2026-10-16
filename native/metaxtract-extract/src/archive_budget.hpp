#pragma once
#include "metaxtract/native/budget.hpp"
#include <cstddef>

namespace metaxtract::native::formats {
class ArchiveBudgetScope final {
    ParserBudget* previous;
    std::size_t previous_emergency;
public:
    static constexpr std::size_t emergency_bytes = 64 * 1024;
    explicit ArchiveBudgetScope(ParserBudget& budget) noexcept;
    ~ArchiveBudgetScope();
    ArchiveBudgetScope(const ArchiveBudgetScope&) = delete;
    ArchiveBudgetScope& operator=(const ArchiveBudgetScope&) = delete;
};
}
