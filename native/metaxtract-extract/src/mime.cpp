#include "mime.hpp"

#include <array>
#include <map>

namespace metaxtract::native::formats {
namespace {
bool space(const char c) { return c == ' ' || c == '\t' || c == '\r' || c == '\n'; }
std::string_view trim(std::string_view text) {
    while (!text.empty() && space(text.front())) { text.remove_prefix(1); }
    while (!text.empty() && space(text.back())) { text.remove_suffix(1); }
    return text;
}
int digit(const char c) {
    if (c >= '0' && c <= '9') { return c - '0'; }
    if (c >= 'a' && c <= 'f') { return c - 'a' + 10; }
    if (c >= 'A' && c <= 'F') { return c - 'A' + 10; }
    return -1;
}
void allocate(Reader& reader, const std::size_t size) {
    if (!reader.context.budget().reserve_allocation(4ULL * size + 64)) { throw Failure{ParseStatus::Internal, "MIME allocation budget exceeded", 0}; }
}
std::string converted(Reader& reader, const std::span<const std::uint8_t> bytes, const std::string_view charset) {
    auto result = decode_text(reader, bytes, charset);
    if (!result.supported) { throw Failure{ParseStatus::Unsupported, "Unsupported MIME charset; original bytes retained", 0}; }
    if (!result.valid) { throw Failure{ParseStatus::Malformed, "Invalid MIME text encoding", 0}; }
    return result.value;
}
std::vector<std::uint8_t> percent(Reader& reader, const std::string_view input) {
    allocate(reader, input.size());
    std::vector<std::uint8_t> output;
    for (std::size_t at = 0; at < input.size(); ++at) {
        reader.checkpoint();
        if (input[at] != '%') { output.push_back(static_cast<std::uint8_t>(input[at])); continue; }
        if (input.size() - at < 3 || digit(input[at + 1]) < 0 || digit(input[at + 2]) < 0) {
            throw Failure{ParseStatus::Malformed, "Invalid RFC2231 percent escape", 0};
        }
        output.push_back(static_cast<std::uint8_t>(digit(input[at + 1]) * 16 + digit(input[at + 2]))); at += 2;
    }
    return output;
}
}

std::vector<std::uint8_t> decode_base64(Reader& reader, const std::string_view input) {
    allocate(reader, input.size());
    std::vector<std::uint8_t> output;
    std::array<int, 4> group{};
    unsigned count = 0;
    bool final = false;
    for (const auto c : input) {
        reader.checkpoint();
        if (space(c)) { continue; }
        if (final) { throw Failure{ParseStatus::Malformed, "Data after MIME base64 padding", 0}; }
        int value = -1;
        if (c >= 'A' && c <= 'Z') { value = c - 'A'; }
        else if (c >= 'a' && c <= 'z') { value = c - 'a' + 26; }
        else if (c >= '0' && c <= '9') { value = c - '0' + 52; }
        else if (c == '+') { value = 62; }
        else if (c == '/') { value = 63; }
        else if (c == '=') { value = -2; }
        else { throw Failure{ParseStatus::Malformed, "Invalid MIME base64 alphabet", 0}; }
        group[count++] = value;
        if (count != 4) { continue; }
        if (group[0] < 0 || group[1] < 0 || (group[2] == -2 && group[3] != -2)) {
            throw Failure{ParseStatus::Malformed, "Invalid MIME base64 padding placement", 0};
        }
        output.push_back(static_cast<std::uint8_t>((group[0] << 2) | (group[1] >> 4)));
        if (group[2] == -2) {
            if ((group[1] & 15) != 0) { throw Failure{ParseStatus::Malformed, "Nonzero MIME base64 pad bits", 0}; }
            final = true;
        } else {
            output.push_back(static_cast<std::uint8_t>((group[1] << 4) | (group[2] >> 2)));
            if (group[3] == -2) {
                if ((group[2] & 3) != 0) { throw Failure{ParseStatus::Malformed, "Nonzero MIME base64 pad bits", 0}; }
                final = true;
            } else { output.push_back(static_cast<std::uint8_t>((group[2] << 6) | group[3])); }
        }
        count = 0;
    }
    if (count != 0) { throw Failure{ParseStatus::Malformed, "Incomplete MIME base64 quantum", 0}; }
    if (!reader.context.budget().consume_decoded(output.size())) { throw Failure{ParseStatus::Internal, "MIME decoded budget exceeded", 0}; }
    return output;
}

std::vector<std::uint8_t> decode_quoted_printable(Reader& reader, const std::string_view input, const bool word) {
    allocate(reader, input.size());
    std::vector<std::uint8_t> output;
    for (std::size_t at = 0; at < input.size(); ++at) {
        reader.checkpoint();
        const auto c = input[at];
        if (word && c == '_') { output.push_back(' '); }
        else if (!word && (c == ' ' || c == '\t')) {
            auto end = at;
            while (end < input.size() && (input[end] == ' ' || input[end] == '\t')) { ++end; }
            if (end == input.size() || input[end] == '\r' || input[end] == '\n') { at = end - 1; }
            else { output.push_back(static_cast<std::uint8_t>(c)); }
        } else if (c != '=') { output.push_back(static_cast<std::uint8_t>(c)); }
        else if (!word && at + 2 < input.size() && input[at + 1] == '\r' && input[at + 2] == '\n') { at += 2; }
        else if (!word && at + 1 < input.size() && input[at + 1] == '\n') {
            reader.diagnostic("noncanonical_qp_break", "LF-only quoted-printable soft break was accepted", 0); ++at;
        } else {
            if (input.size() - at < 3 || digit(input[at + 1]) < 0 || digit(input[at + 2]) < 0) {
                throw Failure{ParseStatus::Malformed, "Invalid quoted-printable escape", 0};
            }
            output.push_back(static_cast<std::uint8_t>(digit(input[at + 1]) * 16 + digit(input[at + 2]))); at += 2;
        }
    }
    if (!reader.context.budget().consume_decoded(output.size())) { throw Failure{ParseStatus::Internal, "MIME decoded budget exceeded", 0}; }
    return output;
}

std::string decode_header_words(Reader& reader, const std::string_view input, const std::string& packet) {
    allocate(reader, input.size());
    std::string output;
    std::size_t at = 0;
    bool previous_word = false;
    while (at < input.size()) {
        reader.entry(); reader.checkpoint();
        const auto start = input.find("=?", at);
        if (start == std::string_view::npos) { output += converted(reader, std::span(reinterpret_cast<const std::uint8_t*>(input.data() + at), input.size() - at), "utf-8"); break; }
        const auto charset_end = input.find('?', start + 2);
        if (charset_end == std::string_view::npos || charset_end + 3 > input.size()) { throw Failure{ParseStatus::Malformed, "Incomplete RFC2047 encoded word", 0}; }
        const auto encoding_end = input.find('?', charset_end + 1);
        const auto end = encoding_end == std::string_view::npos ? encoding_end : input.find("?=", encoding_end + 1);
        if (end == std::string_view::npos || encoding_end != charset_end + 2) { throw Failure{ParseStatus::Malformed, "Invalid RFC2047 encoded word structure", 0}; }
        const auto plain = input.substr(at, start - at);
        if (!previous_word || std::any_of(plain.begin(), plain.end(), [](const auto c) { return !space(c); })) {
            output += converted(reader, std::span(reinterpret_cast<const std::uint8_t*>(plain.data()), plain.size()), "utf-8");
        }
        auto charset = input.substr(start + 2, charset_end - start - 2);
        const auto language = charset.find('*');
        if (language != std::string_view::npos) {
            reader.decoded("encoded_word_language", ValueKind::DecodedBytes, std::string(charset.substr(language + 1)), packet, 3, "ASCII");
            charset = charset.substr(0, language);
        }
        const auto encoded = input.substr(encoding_end + 1, end - encoding_end - 1);
        const auto mode = input[charset_end + 1];
        const auto bytes = mode == 'b' || mode == 'B' ? decode_base64(reader, encoded) :
            mode == 'q' || mode == 'Q' ? decode_quoted_printable(reader, encoded, true) :
            throw Failure{ParseStatus::Malformed, "Unknown RFC2047 word encoding", 0};
        reader.decoded("encoded_word_charset", ValueKind::DecodedBytes, std::string(charset), packet, 3, "ASCII");
        output += converted(reader, bytes, charset);
        at = end + 2; previous_word = true;
    }
    return output;
}

MimeType parse_mime_type(Reader& reader, const std::string_view original, const std::string& packet) {
    allocate(reader, original.size());
    std::string cleaned;
    bool quoted = false, escaped = false;
    std::uint32_t comment_depth = 0;
    for (const auto c : original) {
        reader.checkpoint();
        if (escaped) {
            if (comment_depth == 0) { cleaned.push_back(c); }
            escaped = false; continue;
        }
        if (c == '\\' && (quoted || comment_depth != 0)) {
            if (comment_depth == 0) { cleaned.push_back(c); }
            escaped = true; continue;
        }
        if (c == '"' && comment_depth == 0) { quoted = !quoted; cleaned.push_back(c); continue; }
        if (!quoted && c == '(') {
            if (comment_depth == 0 && !cleaned.empty() && !space(cleaned.back())) { cleaned.push_back(' '); }
            ++comment_depth;
            if (!reader.context.budget().check_depth(comment_depth)) { throw Failure{ParseStatus::Internal, "MIME comment depth budget exceeded", 0}; }
            continue;
        }
        if (!quoted && c == ')' && comment_depth != 0) { --comment_depth; continue; }
        if (comment_depth == 0) { cleaned.push_back(c); }
    }
    if (comment_depth != 0 || quoted || escaped) { throw Failure{ParseStatus::Malformed, "Unterminated MIME comment or quoted string", 0}; }
    const std::string_view input = cleaned;
    MimeType result;
    std::size_t at = 0;
    const auto semicolon = input.find(';');
    const auto atom = [](const std::string_view token) {
        constexpr std::string_view special("()<>@,;:\\\"/[]?=");
        return !token.empty() && std::all_of(token.begin(), token.end(), [special](const auto c) {
            return static_cast<unsigned char>(c) > 0x20 && static_cast<unsigned char>(c) < 0x7f && special.find(c) == std::string_view::npos;
        });
    };
    const auto token = trim(input.substr(0, semicolon));
    const auto slash = token.find('/');
    if (slash == std::string_view::npos) {
        if (!atom(token)) { throw Failure{ParseStatus::Malformed, "Invalid MIME disposition token", 0}; }
        result.token = ascii_lower(token);
    } else {
        const auto left = trim(token.substr(0, slash)), right = trim(token.substr(slash + 1));
        if (!atom(left) || !atom(right)) { throw Failure{ParseStatus::Malformed, "Invalid MIME media type token", 0}; }
        result.token = ascii_lower(left) + "/" + ascii_lower(right);
    }
    if (semicolon == std::string_view::npos) { return result; }
    at = semicolon + 1;
    while (at < input.size()) {
        reader.entry(); reader.checkpoint();
        while (at < input.size() && space(input[at])) { ++at; }
        if (at == input.size()) { break; }
        const auto equal = input.find('=', at);
        if (equal == std::string_view::npos || input.substr(at, equal - at).find(';') != std::string_view::npos) { throw Failure{ParseStatus::Malformed, "MIME parameter has no value", 0}; }
        const auto name = ascii_lower(trim(input.substr(at, equal - at)));
        if (!atom(name)) {
            throw Failure{ParseStatus::Malformed, "Invalid MIME parameter name", 0};
        }
        at = equal + 1;
        while (at < input.size() && space(input[at])) { ++at; }
        std::string value;
        if (at < input.size() && input[at] == '"') {
            ++at; bool closed = false;
            while (at < input.size()) {
                const auto c = input[at++];
                if (c == '"') { closed = true; break; }
                if (c == '\\') {
                    if (at == input.size()) { throw Failure{ParseStatus::Malformed, "Unfinished MIME quoted escape", 0}; }
                    value.push_back(input[at++]);
                } else { value.push_back(c); }
            }
            if (!closed) { throw Failure{ParseStatus::Malformed, "Unfinished MIME quoted parameter", 0}; }
            while (at < input.size() && space(input[at])) { ++at; }
            if (at < input.size() && input[at] != ';') { throw Failure{ParseStatus::Malformed, "Data after MIME quoted parameter", 0}; }
        } else {
            const auto end = input.find(';', at);
            value = trim(input.substr(at, end == std::string_view::npos ? input.size() - at : end - at));
            if (!atom(value)) { throw Failure{ParseStatus::Malformed, "Invalid unquoted MIME parameter token", 0}; }
            at = end == std::string_view::npos ? input.size() : end;
        }
        reader.decoded("parameter_name", ValueKind::Text, name, packet);
        reader.decoded("parameter_bytes", ValueKind::DecodedBytes, value, packet, 3, "binary");
        if (!reader.context.budget().reserve_allocation(128 + 4 * (name.size() + value.size()))) { throw Failure{ParseStatus::Internal, "Parameter allocation budget exceeded", 0}; }
        result.parameters.push_back({name, std::move(value)});
        if (at < input.size()) { ++at; }
    }
    return result;
}

std::string parameter_value(Reader& reader, const MimeType& type, const std::string_view name, const std::string& packet) {
    const auto plain_name = std::string(name), extended_name = plain_name + "*";
    std::string plain, single;
    bool have_plain = false, have_single = false;
    struct Segment { std::string value; bool encoded; };
    std::map<std::uint64_t, Segment> segments;
    for (const auto& parameter : type.parameters) {
        if (parameter.name == plain_name) {
            if (have_plain) { throw Failure{ParseStatus::Malformed, "Duplicate MIME parameter", 0}; }
            plain = parameter.value; have_plain = true;
        } else if (parameter.name == extended_name) {
            if (have_single) { throw Failure{ParseStatus::Malformed, "Duplicate extended MIME parameter", 0}; }
            single = parameter.value; have_single = true;
        } else if (parameter.name.starts_with(extended_name)) {
            auto index = std::string_view(parameter.name).substr(extended_name.size());
            const bool encoded = !index.empty() && index.back() == '*';
            if (encoded) { index.remove_suffix(1); }
            if (index.empty()) { continue; }
            std::uint64_t number = 0;
            for (const auto c : index) {
                if (c < '0' || c > '9' || number > 100000) { throw Failure{ParseStatus::Malformed, "Invalid MIME continuation index", 0}; }
                number = number * 10 + static_cast<unsigned>(c - '0');
            }
            allocate(reader, parameter.value.size() + 128);
            if (!segments.emplace(number, Segment{parameter.value, encoded}).second) { throw Failure{ParseStatus::Malformed, "Duplicate MIME continuation segment", 0}; }
        }
    }
    if (have_single && !segments.empty()) { throw Failure{ParseStatus::Malformed, "Conflicting RFC2231 continuation and single extended parameter", 0}; }
    if (!have_single && segments.empty()) {
        if (!have_plain) { return {}; }
        return converted(reader, std::span(reinterpret_cast<const std::uint8_t*>(plain.data()), plain.size()), "utf-8");
    }
    if (have_plain) { reader.diagnostic("parameter_fallback", "Extended MIME parameter and ASCII fallback are both retained", 0); }
    std::vector<std::uint8_t> bytes;
    std::string charset = "utf-8", language;
    const auto first = [&](const std::string& value) {
        const auto a = value.find('\''), b = a == std::string::npos ? a : value.find('\'', a + 1);
        if (a == std::string::npos || b == std::string::npos) { throw Failure{ParseStatus::Malformed, "RFC2231 charset/language delimiters missing", 0}; }
        charset = value.substr(0, a); language = value.substr(a + 1, b - a - 1);
        return value.substr(b + 1);
    };
    if (have_single) { bytes = percent(reader, first(single)); }
    else {
        std::uint64_t expected = 0;
        for (const auto& [index, segment] : segments) {
            reader.entry();
            if (index != expected++) { throw Failure{ParseStatus::Malformed, "Missing RFC2231 continuation segment", 0}; }
            const auto value = index == 0 && segment.encoded ? first(segment.value) : segment.value;
            const auto part = segment.encoded ? percent(reader, value) : std::vector<std::uint8_t>(value.begin(), value.end());
            allocate(reader, part.size()); bytes.insert(bytes.end(), part.begin(), part.end());
        }
    }
    reader.decoded("parameter_charset", ValueKind::DecodedBytes, charset, packet, 3, "ASCII");
    reader.decoded("parameter_language", ValueKind::DecodedBytes, language, packet, 3, "ASCII");
    return converted(reader, bytes, charset);
}
}
