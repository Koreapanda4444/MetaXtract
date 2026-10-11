#pragma once

#include "metaxtract/native/registry.hpp"

#include <algorithm>
#include <cstring>
#include <iomanip>
#include <limits>
#include <sstream>
#include <span>
#include <utility>

namespace metaxtract::native::formats {

struct Failure final {
    ParseStatus status;
    std::string message;
    std::uint64_t offset;
};

inline std::uint64_t integer(const std::span<const std::uint8_t> bytes, const std::size_t at, const unsigned width, const bool little = false) {
    std::uint64_t value = 0;
    for (unsigned index = 0; index < width; ++index) {
        value |= static_cast<std::uint64_t>(bytes[at + index]) << (8U * (little ? index : width - index - 1));
    }
    return value;
}

inline bool starts(const std::span<const std::uint8_t> bytes, const std::string_view prefix, const std::size_t at = 0) {
    return at <= bytes.size() && prefix.size() <= bytes.size() - at &&
        std::memcmp(bytes.data() + at, prefix.data(), prefix.size()) == 0;
}

inline std::string hex_key(const std::uint64_t value) {
    std::ostringstream text;
    text << "tag_0x" << std::hex << value;
    return text.str();
}

inline std::string path_component(std::string_view name) {
    constexpr char hex[] = "0123456789abcdef";
    std::string result;
    for (const auto c : name) {
        const auto byte = static_cast<unsigned char>(c);
        if ((byte >= 'a' && byte <= 'z') || (byte >= 'A' && byte <= 'Z') || (byte >= '0' && byte <= '9') || byte == '-') { result.push_back(c); }
        else { result.push_back('_'); result.push_back(hex[byte >> 4U]); result.push_back(hex[byte & 15U]); }
    }
    return result;
}

class Reader final {
    std::span<const std::uint8_t> memory_ = {};
    bool decoded_input_ = false;
    std::string packet_ = {};
public:
    ParserContext& context;
    std::uint64_t base;
    std::uint64_t size;
    std::string group;

    Reader(ParserContext& owner, const std::uint64_t start, const std::uint64_t length, std::string name)
        : context(owner), base(start), size(length), group(std::move(name)) {
        if (start > owner.source_size() || length > owner.source_size() - start) {
            throw Failure{ParseStatus::Malformed, "Container range escapes its source", std::min(start, owner.source_size())};
        }
    }

    Reader(ParserContext& owner, std::span<const std::uint8_t> data, std::string name, std::string packet)
        : memory_(data), decoded_input_(true), packet_(std::move(packet)), context(owner),
          base(0), size(data.size()), group(std::move(name)) {}

    Reader window(std::uint64_t at, std::uint64_t length, std::string name) const {
        require(at, length);
        return decoded_input_ ? Reader(context, memory_.subspan(static_cast<std::size_t>(at), static_cast<std::size_t>(length)), std::move(name), packet_)
            : Reader(context, base + at, length, std::move(name));
    }

    void require(const std::uint64_t at, const std::uint64_t length) const {
        if (at > size || length > size - at) {
            throw Failure{ParseStatus::Malformed, "Declared structure exceeds its container", base + std::min(at, size)};
        }
    }

    void checkpoint() const {
        if (!context.budget().checkpoint()) { throw Failure{ParseStatus::Internal, "Parser budget exhausted", base}; }
    }

    void entry() const {
        if (!context.budget().add_entries(1)) { throw Failure{ParseStatus::Internal, "Container entry limit exceeded", base}; }
    }

    std::vector<std::uint8_t> read(const std::uint64_t at, const std::uint64_t length) const {
        require(at, length);
        if (!context.budget().reserve_allocation(length)) {
            throw Failure{ParseStatus::Internal, "Read allocation limit exceeded", base + at};
        }
        std::vector<std::uint8_t> result;
        if (decoded_input_) {
            const auto bytes = memory_.subspan(static_cast<std::size_t>(at), static_cast<std::size_t>(length));
            return {bytes.begin(), bytes.end()};
        }
        result.reserve(static_cast<std::size_t>(length));
        std::uint64_t position = 0;
        while (position < length) {
            const auto count = static_cast<std::uint32_t>(std::min<std::uint64_t>(length - position, 65536));
            auto chunk = context.read_at(base + at + position, count);
            if (chunk.status != ReadStatus::Ok) {
                throw Failure{chunk.status == ReadStatus::OutOfRange ? ParseStatus::OutOfRange :
                    chunk.status == ReadStatus::RequestTooLarge ? ParseStatus::InvalidRequest :
                    chunk.status == ReadStatus::BudgetExhausted ? static_cast<ParseStatus>(BudgetStatus::ReadBytes) :
                    chunk.status == ReadStatus::Internal ? ParseStatus::Internal :
                    chunk.status == ReadStatus::AllocationFailure ? ParseStatus::AllocationFailure : ParseStatus::ReadFailure,
                    "Could not read the declared source range", base + at + position};
            }
            result.insert(result.end(), chunk.bytes.begin(), chunk.bytes.end());
            position += count;
        }
        return result;
    }

    void put(const std::string& key, const ValueKind kind, std::string value,
             const std::uint64_t at, std::vector<std::uint8_t> encoded, const std::string& encoding = "",
             const std::uint64_t tag = 0, const bool numeric = false) const {
        const auto length = encoded.size();
        require(at, length);
        if (!context.observe({
            .name_space = group, .key = key, .kind = kind, .value = std::move(value),
            .encoded = std::move(encoded), .encoding = encoding, .offset = base + at,
            .length = decoded_input_ ? 0 : length, .located = !decoded_input_, .tag_code = tag, .numeric_tag = numeric,
            .method = decoded_input_ ? 3U : 0U, .stream_id = packet_,
        })) { throw Failure{ParseStatus::Internal, "Observation budget exhausted", base + at}; }
    }

    void number(const std::string& key, const std::uint64_t at, const unsigned width, const bool little = false) const {
        auto data = read(at, width);
        const auto value = integer(data, 0, width, little);
        put(key, ValueKind::Unsigned, std::to_string(value), at, std::move(data));
    }

    void derived(const std::string& key, const std::uint64_t value) const {
        if (!context.observe({.name_space = group, .key = key, .kind = ValueKind::Unsigned,
            .value = std::to_string(value), .encoded = {}, .encoding = "", .offset = 0,
            .length = 0, .located = false, .tag_code = 0, .numeric_tag = false,
            .method = decoded_input_ ? 3U : 0U, .stream_id = packet_})) {
            throw Failure{ParseStatus::Internal, "Observation budget exhausted", base};
        }
    }

    void decoded(const std::string& key, ValueKind kind, std::string value,
                 const std::string& packet, std::uint32_t method = 3, const std::string& encoding = "UTF-8") const {
        if (!context.budget().consume_decoded(value.size()) || !context.observe({
            .name_space = group, .key = key, .kind = kind, .value = std::move(value),
            .encoded = {}, .encoding = encoding, .offset = 0, .length = 0, .located = false,
            .tag_code = 0, .numeric_tag = false, .method = method, .stream_id = packet,
        })) { throw Failure{ParseStatus::Internal, "Decoded observation budget exhausted", base}; }
    }

    void diagnostic(const std::string& key, const std::string& message, const std::uint64_t at) const {
        if (!context.observe({.name_space = "diagnostic." + group, .key = key, .kind = ValueKind::Text,
            .value = message, .encoded = {}, .encoding = "UTF-8", .offset = base + std::min(at, size),
            .length = 0, .located = !decoded_input_, .tag_code = 0, .numeric_tag = false,
            .method = decoded_input_ ? 3U : 0U, .stream_id = packet_})) {
            throw Failure{ParseStatus::Internal, "Diagnostic budget exhausted", base};
        }
    }
};

void inspect_tiff(Reader& reader, bool metadata = false, std::uint32_t depth = 0);
void inspect_png(Reader& reader, bool metadata = false);
void inspect_jpeg(Reader& reader, bool metadata = false);
void inspect_webp(Reader& reader, bool metadata = false);
void inspect_gif(Reader& reader, bool metadata = false);
void inspect_bmp(Reader& reader);
void inspect_ico(Reader& reader, bool metadata = false);
void inspect_xmp(Reader& reader);
void inspect_xmp_data(Reader& reader, std::span<const std::uint8_t> bytes, const std::string& packet);
void inspect_icc(Reader& reader);
void inspect_icc_data(Reader& reader, std::span<const std::uint8_t> bytes);
void inspect_iptc(Reader& reader);
void jpeg_metadata(Reader& reader, std::uint8_t code, std::uint64_t at, std::uint64_t length);
void png_metadata(Reader& reader, const std::string& type, std::uint64_t at, std::uint64_t length);
void webp_metadata(Reader& reader, const std::string& type, std::uint64_t at, std::uint64_t length);
void gif_metadata(Reader& reader, std::uint8_t label, std::uint64_t start, std::uint64_t end);
std::vector<std::uint8_t> inflate_metadata(Reader& reader, std::span<const std::uint8_t> bytes);

}
