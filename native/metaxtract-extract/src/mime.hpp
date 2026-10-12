#pragma once

#include "text.hpp"

namespace metaxtract::native::formats {
std::vector<std::uint8_t> decode_base64(Reader& reader, std::string_view input);
std::vector<std::uint8_t> decode_quoted_printable(Reader& reader, std::string_view input, bool word = false);
std::string decode_header_words(Reader& reader, std::string_view input, const std::string& packet);
struct MimeParameter final { std::string name; std::string value; };
struct MimeType final { std::string token; std::vector<MimeParameter> parameters; };
MimeType parse_mime_type(Reader& reader, std::string_view input, const std::string& packet);
std::string parameter_value(Reader& reader, const MimeType& type, std::string_view name, const std::string& packet);
}
