#include "metaxtract/native/registry.hpp"

#include <algorithm>
#include <utility>

namespace metaxtract::native {

bool ParserRegistry::add(std::unique_ptr<Parser> parser) {
    if (!parser || find(parser->capability().id) != nullptr) {
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

ParserRegistry& default_registry() noexcept {
    static ParserRegistry registry;
    return registry;
}

}
