#pragma once

#include "metaxtract/native/input.hpp"

#include <cstdint>
#include <string>
#include <vector>

namespace metaxtract::native {

enum ParserOperation : std::uint32_t {
    Probe = 1U << 0U,
    Extract = 1U << 1U,
    DecodePreview = 1U << 2U,
    EnumerateChildren = 1U << 3U,
};

struct ParserCapability final {
    std::string id;
    std::string name;
    std::string version;
    std::uint32_t operations;
    std::vector<std::string> formats;
};

class Parser {
public:
    virtual ~Parser() = default;

    [[nodiscard]] virtual const ParserCapability& capability() const noexcept = 0;
};

}
