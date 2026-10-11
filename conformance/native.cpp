#include "parser.hpp"
#include "metaxtract/native/core.hpp"
#include "metaxtract/native/registry.hpp"

#include <algorithm>
#include <cstring>
#include <iostream>
#include <limits>
#include <memory>

namespace {
using namespace metaxtract::native;

class Input final : public RandomAccessInput {
public:
    explicit Input(const std::uint8_t opcode) : bytes_{opcode, 10, 20, 30} {}
    std::uint64_t size() const noexcept override { return bytes_.size(); }
    ReadChunk read_at(const std::uint64_t offset, const std::uint32_t requested) const noexcept override {
        if (offset > bytes_.size()) {
            return {.bytes = {}, .status = ReadStatus::OutOfRange};
        }
        try {
            const auto length = std::min<std::uint64_t>(requested, bytes_.size() - offset);
            return {
                .bytes = std::vector<std::uint8_t>(bytes_.begin() + static_cast<std::ptrdiff_t>(offset), bytes_.begin() + static_cast<std::ptrdiff_t>(offset + length)),
                .status = ReadStatus::Ok,
            };
        } catch (...) {
            return {.bytes = {}, .status = ReadStatus::Internal};
        }
    }
private:
    std::vector<std::uint8_t> bytes_;
};

int checks = 0;
bool expect(const bool condition, const char* message) {
    ++checks;
    if (!condition) { std::cerr << "Failed: " << message << '\n'; }
    return condition;
}

}

int main() {
    using namespace metaxtract::native;
    const auto info = core_info();
    bool ok = expect(info.api_revision == kApiRevision && info.schema_revision == kSchemaRevision, "native revisions");
    ok &= expect(std::strcmp(engine_name(), "metaxtract-native") == 0, "native engine identity");
    ok &= expect(default_registry().parsers().empty(), "no production fixture parser");
    ParserRegistry registry;
    ok &= expect(registry.add(std::make_unique<ConformanceParser>()), "parser registration");
    ok &= expect(!registry.add(std::make_unique<ConformanceParser>()) && !registry.add(nullptr), "duplicate/null parser rejection");
    const ResourceLimits limits{
        .max_source_bytes = 4, .max_read_bytes = 4, .max_allocation_bytes = 8,
        .max_decoded_bytes = 4, .max_container_depth = 2, .max_entries = 2,
        .max_observations = 2, .max_wall_time_ms = 1'000,
    };
    const Input normal(0);
    const auto first = run_parser(registry, "conformance.resource_limits", normal, limits);
    ok &= expect(first.status == 0 && first.read_bytes == 4 && first.allocation_bytes == 8, "charged parser reads");
    const auto second = run_parser(registry, "conformance.resource_limits", normal, limits);
    ok &= expect(second.status == 0 && second.read_bytes == 4, "independent parser budgets");
    for (std::uint8_t opcode = 1; opcode <= 8; ++opcode) {
        auto ceiling = limits;
        if (opcode == 8) { ceiling.max_wall_time_ms = 1; }
        const Input input(opcode);
        const auto run = run_parser(registry, "conformance.resource_limits", input, ceiling);
        const std::uint32_t expected[] = {0, 11, 12, 13, 14, 15, 16, 3, 17};
        ok &= expect(run.status == expected[opcode], "parser failure code");
        ok &= expect(run.recoverable == (opcode != 7), "parser failure recoverability");
    }
    auto small_source = limits;
    small_source.max_source_bytes = 3;
    const auto oversized = run_parser(registry, "conformance.resource_limits", normal, small_source);
    ok &= expect(oversized.status == 10 && oversized.read_bytes == 0, "source limit before parser execution");
    ok &= expect(run_parser(registry, "unknown", normal, limits).status == 7, "missing parser error");
    auto invalid = limits;
    invalid.max_wall_time_ms = 0;
    ok &= expect(run_parser(registry, "conformance.resource_limits", normal, invalid).status == 2, "invalid deadline policy");
    ParserBudget overflow(limits, 4);
    ok &= expect(!overflow.reserve_allocation(std::numeric_limits<std::uint64_t>::max()) && overflow.status() == BudgetStatus::AllocationBytes, "allocation overflow prevention");
    ParserContext context(normal, limits);
    ok &= expect(context.read_at(5, 1).status == ReadStatus::OutOfRange && context.budget().read_bytes() == 0, "range check before charging");
    std::cout << "Native conformance: " << checks << " checks\n";
    return ok ? 0 : 1;
}
