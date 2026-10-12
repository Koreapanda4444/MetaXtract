#pragma once

#include "text.hpp"

namespace metaxtract::native::formats {
void inspect_properties(Reader& reader, const std::string& packet);
void inspect_legacy_stream(Reader& reader, const std::string& name, const std::string& packet, std::uint32_t depth);
ParseStatus inspect_compound(Reader& reader, std::uint32_t depth = 1);
}
