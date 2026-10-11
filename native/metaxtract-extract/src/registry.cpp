#include "metaxtract/native/registry.hpp"

#include <algorithm>
#include <utility>

namespace metaxtract::native {

#ifdef METAXTRACT_CONFORMANCE
std::unique_ptr<Parser> conformance_parser();
#endif

bool ParserRegistry::add(std::unique_ptr<Parser> parser) {
    if (!parser || parser->capability().id.empty() || find(parser->capability().id) != nullptr) {
        return false;
    }
    parsers_.push_back(std::move(parser));
    return true;
}

const Parser* ParserRegistry::find(const std::string_view id) const noexcept {
    const auto found = std::find_if(
        parsers_.begin(),
        parsers_.end(),
        [id](const auto& parser) { return parser->capability().id == id; });
    return found == parsers_.end() ? nullptr : found->get();
}

const std::vector<std::unique_ptr<Parser>>& ParserRegistry::parsers() const noexcept {
    return parsers_;
}

const ParserRegistry& default_registry() noexcept {
    static const ParserRegistry registry = [] {
        ParserRegistry result;
#ifdef METAXTRACT_CONFORMANCE
        result.add(conformance_parser());
#endif
        return result;
    }();
    return registry;
}

ParserRun run_parser(
    const ParserRegistry& registry,
    const std::string_view parser_id,
    const RandomAccessInput& source,
    const ResourceLimits limits) noexcept {
    ParserContext context(source, limits);
    auto status = ParseStatus::Complete;
    if (limits.max_wall_time_ms == 0) {
        status = ParseStatus::InvalidRequest;
    } else if (context.budget().checkpoint()) {
        const auto* parser = registry.find(parser_id);
        if (parser == nullptr) {
            status = ParseStatus::Unsupported;
        } else {
            try {
                status = parser->run(context);
            } catch (...) {
                status = ParseStatus::Internal;
            }
        }
        static_cast<void>(context.budget().checkpoint());
    }

    const auto& budget = context.budget();
    const auto code = status == ParseStatus::InvalidRequest
        ? static_cast<std::uint32_t>(status)
        : budget.status() != BudgetStatus::Ok
        ? static_cast<std::uint32_t>(budget.status())
        : static_cast<std::uint32_t>(status);
    return ParserRun{
        .status = code,
        .recoverable = code != 0 && code != 2 && code != 3 && code != 8,
        .read_bytes = budget.read_bytes(),
        .allocation_bytes = budget.allocation_bytes(),
        .decoded_bytes = budget.decoded_bytes(),
        .entries = budget.entries(),
        .observations = budget.observations(),
    };
}

}
