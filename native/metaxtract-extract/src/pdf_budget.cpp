#include "pdf_budget.hpp"
#include "format.hpp"
#include <cstddef>
namespace metaxtract::native::formats::pdf {
namespace {
thread_local ParserBudget* current_budget = nullptr;
thread_local std::uint32_t current_warnings = 0;
}
BufferBudgetScope::BufferBudgetScope(ParserBudget& budget) noexcept
    : previous_(current_budget), previous_warnings_(current_warnings) {
    current_budget = &budget; current_warnings = 0;
}
BufferBudgetScope::~BufferBudgetScope() { current_budget = previous_; current_warnings = previous_warnings_; }
void charge_buffer(std::size_t count) {
    if (!current_budget) { return; }
    if (!current_budget->checkpoint() || count > UINT64_MAX / 2 || !current_budget->consume_decoded(count) ||
        !current_budget->reserve_allocation(static_cast<std::uint64_t>(count) * 2)) {
        throw Failure{ParseStatus::Internal, "QPDF decoded buffer budget exceeded", 0};
    }
}
void charge_warning(std::size_t count) {
    if (!current_budget) { return; }
    if (!current_budget->checkpoint() || !current_budget->add_entries(1) ||
        count > (UINT64_MAX - 1024) / 4 || !current_budget->reserve_allocation(static_cast<std::uint64_t>(count) * 4 + 1024)) {
        throw Failure{ParseStatus::Internal, "QPDF warning budget exceeded", 0};
    }
    if (++current_warnings > 100) { throw Failure{ParseStatus::Malformed, "PDF warning count ceiling exceeded", 0}; }
}
}
extern "C" void metaxtract_qpdf_buffer_charge(std::size_t count) {
    metaxtract::native::formats::pdf::charge_buffer(count);
}
extern "C" void metaxtract_qpdf_warning_charge(std::size_t count) {
    metaxtract::native::formats::pdf::charge_warning(count);
}
