#include "metaxtract/native/budget.hpp"
#include "metaxtract/native/core.hpp"
#include "metaxtract/native/registry.hpp"

#include <cstring>

int main() {
    const auto info = metaxtract::native::core_info();
    if (info.api_revision != metaxtract::native::kApiRevision) {
        return 1;
    }
    if (info.schema_revision != metaxtract::native::kSchemaRevision) {
        return 2;
    }
    if (std::strcmp(metaxtract::native::engine_name(), "metaxtract-native") != 0) {
        return 3;
    }
    if (!metaxtract::native::default_registry().parsers().empty()) {
        return 4;
    }

    const metaxtract::native::ResourceLimits limits{
        .max_source_bytes = 8,
        .max_read_bytes = 4,
        .max_allocation_bytes = 4,
        .max_decoded_bytes = 4,
        .max_container_depth = 2,
        .max_entries = 2,
        .max_observations = 2,
        .max_wall_time_ms = 1'000,
    };
    metaxtract::native::ParserBudget source_budget(limits, 9);
    if (source_budget.status() != metaxtract::native::BudgetStatus::SourceBytes) {
        return 5;
    }
    metaxtract::native::ParserBudget read_budget(limits, 8);
    if (!read_budget.consume_read(4) || read_budget.consume_read(1)) {
        return 6;
    }
    if (read_budget.status() != metaxtract::native::BudgetStatus::ReadBytes) {
        return 7;
    }
    metaxtract::native::ParserBudget depth_budget(limits, 8);
    if (depth_budget.check_depth(3)) {
        return 8;
    }
    if (depth_budget.status() != metaxtract::native::BudgetStatus::ContainerDepth) {
        return 9;
    }
    return 0;
}
