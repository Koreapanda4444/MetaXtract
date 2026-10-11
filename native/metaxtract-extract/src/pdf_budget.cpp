#include "pdf_budget.hpp"
#include "format.hpp"
#include <cstddef>
namespace metaxtract::native::formats::pdf {
namespace { thread_local ParserBudget* current_budget = nullptr; }
BufferBudgetScope::BufferBudgetScope(ParserBudget& budget) noexcept : previous_(current_budget) { current_budget = &budget; }
BufferBudgetScope::~BufferBudgetScope() { current_budget = previous_; }
void charge_buffer(std::size_t count) {
    if (!current_budget) { return; }
    if (count > UINT64_MAX / 2 || !current_budget->consume_decoded(count) ||
        !current_budget->reserve_allocation(static_cast<std::uint64_t>(count) * 2)) {
        throw Failure{ParseStatus::Internal, "QPDF decoded buffer budget exceeded", 0};
    }
}
}
extern "C" void metaxtract_qpdf_buffer_charge(std::size_t count) {
    metaxtract::native::formats::pdf::charge_buffer(count);
}
