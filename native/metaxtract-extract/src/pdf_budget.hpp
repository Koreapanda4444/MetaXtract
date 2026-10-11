#pragma once
#include "metaxtract/native/budget.hpp"
namespace metaxtract::native::formats::pdf {
class BufferBudgetScope final {
    ParserBudget* previous_;
public:
    explicit BufferBudgetScope(ParserBudget& budget) noexcept;
    ~BufferBudgetScope();
    BufferBudgetScope(const BufferBudgetScope&) = delete;
    BufferBudgetScope& operator=(const BufferBudgetScope&) = delete;
};
}
