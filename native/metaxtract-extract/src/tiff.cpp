#include "format.hpp"

#include <bit>
#include <set>

namespace metaxtract::native::formats {
namespace {

const char* tag_name(const std::uint64_t tag) {
    switch (tag) {
        case 256: return "width";
        case 257: return "height";
        case 258: return "bits_per_sample";
        case 259: return "compression";
        case 262: return "photometric_interpretation";
        case 271: return "make";
        case 272: return "model";
        case 274: return "orientation";
        case 277: return "samples_per_pixel";
        case 282: return "x_resolution";
        case 283: return "y_resolution";
        case 296: return "resolution_unit";
        case 305: return "software";
        case 306: return "timestamp";
        case 315: return "artist";
        case 700: return "xmp";
        case 33723: return "iptc";
        case 34675: return "icc_profile";
        default: return nullptr;
    }
}

unsigned type_width(const std::uint64_t type) {
    switch (type) {
        case 1: case 2: case 6: case 7: return 1;
        case 3: case 8: return 2;
        case 4: case 9: case 11: case 13: return 4;
        case 5: case 10: case 12: case 16: case 17: case 18: return 8;
        default: return 0;
    }
}

}

void inspect_tiff(Reader& reader, const bool metadata) {
    static_cast<void>(metadata);
    auto header = reader.read(0, 8);
    const auto little = starts(header, "II");
    if (!little && !starts(header, "MM")) { throw Failure{ParseStatus::Malformed, "Invalid TIFF byte order", reader.base}; }
    const auto magic = integer(header, 2, 2, little);
    const auto big = magic == 43;
    if (magic != 42 && !big) { throw Failure{ParseStatus::Malformed, "Invalid TIFF magic", reader.base + 2}; }
    std::uint64_t offset = integer(header, 4, 4, little);
    if (big) {
        header = reader.read(0, 16);
        if (integer(header, 4, 2, little) != 8 || integer(header, 6, 2, little) != 0) {
            throw Failure{ParseStatus::Malformed, "Invalid BigTIFF offset size", reader.base + 4};
        }
        offset = integer(header, 8, 8, little);
    }
    reader.derived("byte_order_little", little ? 1 : 0);
    reader.derived("bigtiff", big ? 1 : 0);
    std::set<std::uint64_t> visited;
    std::uint32_t directories = 0;
    const auto count_width = big ? 8U : 2U;
    const auto entry_width = big ? 20U : 12U;
    const auto inline_width = big ? 8U : 4U;
    while (offset != 0) {
        reader.checkpoint();
        if (offset < (big ? 16U : 8U)) { throw Failure{ParseStatus::Malformed, "TIFF directory overlaps header", reader.base + offset}; }
        reader.entry();
        if (visited.contains(offset)) { throw Failure{ParseStatus::Malformed, "TIFF directory cycle", reader.base + std::min(offset, reader.size)}; }
        if (!reader.context.budget().reserve_allocation(64)) { throw Failure{ParseStatus::Internal, "Directory allocation budget", reader.base}; }
        visited.insert(offset);
        const auto count_data = reader.read(offset, count_width);
        const auto count = integer(count_data, 0, count_width, little);
        reader.require(offset, count_width + inline_width);
        if (count > (reader.size - offset - count_width - inline_width) / entry_width) {
            throw Failure{ParseStatus::Malformed, "TIFF entry table size overflow", reader.base + offset};
        }
        const auto table_bytes = count * entry_width;
        reader.require(offset + count_width, table_bytes + inline_width);
        Reader directory(reader.context, reader.base, reader.size, reader.group + ".ifd" + std::to_string(directories));
        for (std::uint64_t index = 0; index < count; ++index) {
            directory.entry();
            const auto at = offset + count_width + index * entry_width;
            const auto entry = directory.read(at, entry_width);
            const auto tag = integer(entry, 0, 2, little);
            const auto type = integer(entry, 2, 2, little);
            const auto quantity = integer(entry, 4, big ? 8 : 4, little);
            const auto width = type_width(type);
            if (width == 0) {
                directory.put(hex_key(tag), ValueKind::Bytes, "", at, entry, "", tag, true);
                directory.diagnostic("unknown_tiff_type", "Unknown TIFF type; the complete directory entry is retained", at);
                continue;
            }
            if (quantity > std::numeric_limits<std::uint64_t>::max() / width) { throw Failure{ParseStatus::Malformed, "TIFF value size overflow", directory.base + at}; }
            const auto length = quantity * width;
            const auto value_at = length <= inline_width ? at + (big ? 12 : 8) : integer(entry, big ? 12 : 8, inline_width, little);
            auto raw = directory.read(value_at, length);
            const auto* known = tag_name(tag);
            const auto key = known == nullptr ? hex_key(tag) : std::string(known);
            ValueKind kind = ValueKind::Bytes;
            std::string value;
            std::string encoding;
            if (type != 7 && quantity != 0) {
                const auto output = type == 2 ? quantity : quantity * 32;
                const auto allocation = type == 2 ? quantity * 2 : quantity * 256;
                if (!directory.context.budget().reserve_allocation(allocation) || !directory.context.budget().consume_decoded(output)) {
                    throw Failure{ParseStatus::Internal, "TIFF value decoding budget", directory.base + value_at};
                }
            }
            if (type == 2) {
                kind = ValueKind::Text;
                value.assign(raw.begin(), raw.end());
                while (!value.empty() && value.back() == '\0') { value.pop_back(); }
                encoding = "ASCII";
            } else if (type == 1 || type == 3 || type == 4 || type == 13 || type == 16 || type == 18) {
                kind = quantity == 1 ? ValueKind::Unsigned : ValueKind::UnsignedSequence;
                for (std::uint64_t part = 0; part < quantity; ++part) {
                    if (part % 1024 == 0) { directory.checkpoint(); }
                    if (!value.empty()) { value += ','; }
                    value += std::to_string(integer(raw, static_cast<std::size_t>(part * width), width, little));
                }
                if (quantity == 0) { kind = ValueKind::Bytes; }
            } else if (type == 5 || type == 10) {
                kind = quantity == 1 ? ValueKind::Rational : ValueKind::RationalSequence;
                for (std::uint64_t part = 0; part < quantity; ++part) {
                    if (part % 1024 == 0) { directory.checkpoint(); }
                    if (!value.empty()) { value += ','; }
                    const auto numerator = integer(raw, static_cast<std::size_t>(part * 8), 4, little);
                    const auto denominator = integer(raw, static_cast<std::size_t>(part * 8 + 4), 4, little);
                    value += std::to_string(type == 10 ? static_cast<std::int64_t>(static_cast<std::int32_t>(numerator)) : static_cast<std::int64_t>(numerator));
                    value += '/' + std::to_string(type == 10 ? static_cast<std::int64_t>(static_cast<std::int32_t>(denominator)) : static_cast<std::int64_t>(denominator));
                    if (denominator == 0) { directory.diagnostic("zero_denominator", "TIFF rational has a zero denominator", value_at + part * 8); }
                }
                if (quantity == 0) { kind = ValueKind::Bytes; }
            }
            directory.put(key, kind, std::move(value), value_at, std::move(raw), encoding, tag, true);
        }
        const auto next = reader.read(offset + count_width + table_bytes, inline_width);
        offset = integer(next, 0, inline_width, little);
        ++directories;
    }
    reader.derived("directory_count", directories);
}

}
