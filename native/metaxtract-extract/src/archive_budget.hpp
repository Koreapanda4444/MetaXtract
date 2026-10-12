#pragma once
#include "metaxtract/native/budget.hpp"

namespace metaxtract::native::formats {
class ArchiveBudgetScope final {
    ParserBudget* previous;
public:
    explicit ArchiveBudgetScope(ParserBudget& budget) noexcept;
    ~ArchiveBudgetScope();
    ArchiveBudgetScope(const ArchiveBudgetScope&) = delete;
    ArchiveBudgetScope& operator=(const ArchiveBudgetScope&) = delete;
};
}
