#pragma once

#include "format.hpp"

namespace metaxtract::native::formats {
struct TextResult final {
    bool supported = true;
    bool valid = true;
    std::string value;
};
TextResult decode_text(Reader& reader, std::span<const std::uint8_t> bytes, std::string_view charset);
std::string ascii_lower(std::string_view text);
}
