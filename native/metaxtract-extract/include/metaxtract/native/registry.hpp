#pragma once

#include "metaxtract/native/parser.hpp"

#include <memory>
#include <string_view>
#include <vector>

namespace metaxtract::native {

class ParserRegistry final {
public:
    bool add(std::unique_ptr<Parser> parser);

    [[nodiscard]] const Parser* find(std::string_view id) const noexcept;
    [[nodiscard]] const std::vector<std::unique_ptr<Parser>>& parsers() const noexcept;

private:
    std::vector<std::unique_ptr<Parser>> parsers_;
};

[[nodiscard]] ParserRegistry& default_registry() noexcept;

}
