#pragma once

#include "metaxtract/native/parser.hpp"

#include <string>
#include <vector>

namespace metaxtract::native {

enum class DetectionState : std::uint32_t {
    Signature = 0,
    HeaderValidated = 1,
    Truncated = 2,
    Invalid = 3,
};

struct ContentCandidate final {
    std::string format;
    std::string mime;
    std::string container;
    DetectionState state;
    std::uint64_t offset;
    std::uint64_t length;
    std::string detail;
};

struct DetectionFailure final {
    std::uint32_t status;
};

[[nodiscard]] std::vector<ContentCandidate> detect_content(ParserContext& context);

}
