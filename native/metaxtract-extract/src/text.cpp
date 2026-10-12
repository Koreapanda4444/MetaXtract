#include "text.hpp"

#include <array>

namespace metaxtract::native::formats {
namespace {
void append_utf8(std::string& out, const std::uint32_t code) {
    if (code < 0x80) { out.push_back(static_cast<char>(code)); }
    else if (code < 0x800) {
        out.push_back(static_cast<char>(0xc0U | (code >> 6U)));
        out.push_back(static_cast<char>(0x80U | (code & 63U)));
    } else if (code < 0x10000) {
        out.push_back(static_cast<char>(0xe0U | (code >> 12U)));
        out.push_back(static_cast<char>(0x80U | ((code >> 6U) & 63U)));
        out.push_back(static_cast<char>(0x80U | (code & 63U)));
    } else {
        out.push_back(static_cast<char>(0xf0U | (code >> 18U)));
        out.push_back(static_cast<char>(0x80U | ((code >> 12U) & 63U)));
        out.push_back(static_cast<char>(0x80U | ((code >> 6U) & 63U)));
        out.push_back(static_cast<char>(0x80U | (code & 63U)));
    }
}
}

std::string ascii_lower(const std::string_view text) {
    std::string result(text);
    for (auto& c : result) {
        if (c >= 'A' && c <= 'Z') { c = static_cast<char>(c + ('a' - 'A')); }
    }
    return result;
}

TextResult decode_text(Reader& reader, const std::span<const std::uint8_t> bytes, const std::string_view charset) {
    const auto name = ascii_lower(charset);
    const bool utf8 = name == "utf-8" || name == "utf8" || name == "65001";
    const bool utf16 = name == "utf-16" || name == "utf-16le" || name == "utf-16be" || name == "1200" || name == "1201";
    const bool ascii = name == "ascii" || name == "us-ascii" || name == "20127";
    const bool latin = name == "iso-8859-1" || name == "latin1" || name == "latin-1" || name == "28591";
    const bool cp1252 = name == "windows-1252" || name == "cp1252" || name == "1252";
    if (!utf8 && !utf16 && !ascii && !latin && !cp1252) { return {false, true, {}}; }
    if (bytes.size() > (std::numeric_limits<std::uint64_t>::max() - 64) / 4 ||
        !reader.context.budget().reserve_allocation(4 * bytes.size() + 64)) {
        throw Failure{ParseStatus::Internal, "Text allocation budget exceeded", reader.base};
    }
    TextResult result;
    result.value.reserve(bytes.size());
    if (utf16) {
        bool little = name != "utf-16be" && name != "1201";
        std::size_t at = 0;
        if (bytes.size() >= 2 && bytes[0] == 0xff && bytes[1] == 0xfe) { little = true; at = 2; }
        else if (bytes.size() >= 2 && bytes[0] == 0xfe && bytes[1] == 0xff) { little = false; at = 2; }
        else if (name == "utf-16") { return {true, false, {}}; }
        if ((bytes.size() - at) % 2 != 0) { return {true, false, {}}; }
        while (at < bytes.size()) {
            reader.checkpoint();
            auto code = static_cast<std::uint32_t>(integer(bytes, at, 2, little)); at += 2;
            if (code >= 0xd800 && code <= 0xdbff) {
                if (bytes.size() - at < 2) { return {true, false, {}}; }
                const auto low = static_cast<std::uint32_t>(integer(bytes, at, 2, little)); at += 2;
                if (low < 0xdc00 || low > 0xdfff) { return {true, false, {}}; }
                code = 0x10000U + ((code - 0xd800U) << 10U) + low - 0xdc00U;
            } else if (code >= 0xdc00 && code <= 0xdfff) { return {true, false, {}}; }
            append_utf8(result.value, code);
        }
        return result;
    }
    constexpr std::array<std::uint16_t, 32> mapping = {
        0x20ac,0,0x201a,0x0192,0x201e,0x2026,0x2020,0x2021,0x02c6,0x2030,0x0160,0x2039,0x0152,0,0x017d,0,
        0,0x2018,0x2019,0x201c,0x201d,0x2022,0x2013,0x2014,0x02dc,0x2122,0x0161,0x203a,0x0153,0,0x017e,0x0178};
    for (std::size_t at = 0; at < bytes.size();) {
        reader.checkpoint();
        std::uint32_t code = bytes[at++];
        if (utf8 && code >= 0x80) {
            unsigned extra = 0; std::uint32_t minimum = 0;
            if (code >= 0xc2 && code <= 0xdf) { extra = 1; code &= 31U; minimum = 0x80; }
            else if (code >= 0xe0 && code <= 0xef) { extra = 2; code &= 15U; minimum = 0x800; }
            else if (code >= 0xf0 && code <= 0xf4) { extra = 3; code &= 7U; minimum = 0x10000; }
            else { return {true, false, {}}; }
            if (extra > bytes.size() - at) { return {true, false, {}}; }
            for (unsigned index = 0; index < extra; ++index) {
                const auto byte = bytes[at++];
                if ((byte & 0xc0U) != 0x80U) { return {true, false, {}}; }
                code = (code << 6U) | (byte & 63U);
            }
            if (code < minimum || code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff)) { return {true, false, {}}; }
        } else if (ascii && code >= 0x80) { return {true, false, {}}; }
        else if (cp1252 && code >= 0x80 && code < 0xa0) {
            code = mapping[code - 0x80];
            if (code == 0) { return {true, false, {}}; }
        }
        append_utf8(result.value, code);
    }
    return result;
}
}
