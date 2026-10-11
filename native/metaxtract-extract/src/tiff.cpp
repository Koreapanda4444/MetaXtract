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
        case 330: return "sub_ifds";
        case 513: return "thumbnail_offset";
        case 514: return "thumbnail_length";
        case 700: return "xmp";
        case 33723: return "iptc";
        case 34665: return "exif_ifd";
        case 34675: return "icc_profile";
        case 34853: return "gps_ifd";
        case 33434: return "exposure_time";
        case 33437: return "f_number";
        case 34855: return "iso";
        case 36864: return "exif_version";
        case 36867: return "datetime_original";
        case 36868: return "datetime_digitized";
        case 36880: return "offset_time";
        case 36881: return "offset_time_original";
        case 36882: return "offset_time_digitized";
        case 37386: return "focal_length";
        case 37500: return "maker_note";
        case 37510: return "user_comment";
        case 37521: return "subsec_time_original";
        case 40961: return "color_space";
        case 40962: return "pixel_x_dimension";
        case 40963: return "pixel_y_dimension";
        case 40965: return "interop_ifd";
        case 42033: return "body_serial_number";
        case 42036: return "lens_model";
        case 42037: return "lens_serial_number";
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

void inspect_tiff(Reader& reader, const bool metadata, const std::uint32_t depth) {
    if (!reader.context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "TIFF metadata depth", reader.base}; }
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
    struct Directory { std::uint64_t at; std::string group; std::uint32_t depth; };
    std::vector<Directory> pending;
    if (offset != 0) { pending.push_back({offset, reader.group + ".ifd0", depth}); }
    while (!pending.empty()) {
        const auto item = std::move(pending.back()); pending.pop_back();
        offset = item.at;
        if (!reader.context.budget().check_depth(item.depth)) { throw Failure{ParseStatus::Internal, "TIFF directory depth limit", reader.base + offset}; }
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
        auto directory = reader.window(0, reader.size, item.group);
        std::uint64_t thumbnail_offset = 0, thumbnail_length = 0;
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
            } else if (type == 6 || type == 8 || type == 9 || type == 17) {
                kind = quantity == 1 ? ValueKind::Signed : ValueKind::SignedSequence;
                for (std::uint64_t part = 0; part < quantity; ++part) {
                    if (part % 1024 == 0) { directory.checkpoint(); }
                    if (!value.empty()) { value += ','; }
                    auto number = integer(raw, static_cast<std::size_t>(part * width), width, little);
                    if (width < 8 && (number & (1ULL << (width * 8 - 1))) != 0) { number |= (~0ULL) << (width * 8); }
                    value += std::to_string(std::bit_cast<std::int64_t>(number));
                }
                if (quantity == 0) { kind = ValueKind::Bytes; }
            } else if (type == 11 || type == 12) {
                kind = quantity == 1 ? ValueKind::Float : ValueKind::FloatSequence;
                for (std::uint64_t part = 0; part < quantity; ++part) {
                    if (part % 1024 == 0) { directory.checkpoint(); }
                    if (!value.empty()) { value += ','; }
                    const auto bits = integer(raw, static_cast<std::size_t>(part * width), width, little);
                    std::ostringstream number; number << std::setprecision(17) << (type == 11 ? static_cast<double>(std::bit_cast<float>(static_cast<std::uint32_t>(bits))) : std::bit_cast<double>(bits));
                    value += number.str();
                }
                if (quantity == 0) { kind = ValueKind::Bytes; }
            }
            if (metadata && (tag == 330 || tag == 34665 || tag == 34853 || tag == 40965) && (type == 4 || type == 13 || type == 16 || type == 18)) {
                for (std::uint64_t part = 0; part < quantity; ++part) {
                    directory.entry();
                    const auto pointer = integer(raw, static_cast<std::size_t>(part * width), width, little);
                    if (pointer == 0) { continue; }
                    if (!directory.context.budget().reserve_allocation(512)) { throw Failure{ParseStatus::Internal, "TIFF subdirectory allocation", directory.base}; }
                    const auto suffix = tag == 34665 ? ".exif" : tag == 34853 ? ".gps" : tag == 40965 ? ".interop" : ".subifd";
                    pending.push_back({pointer, directory.group + suffix + std::to_string(part), item.depth + 1});
                }
            }
            if (quantity == 1 && kind == ValueKind::Unsigned) {
                if (tag == 513) { thumbnail_offset = integer(raw, 0, width, little); }
                if (tag == 514) { thumbnail_length = integer(raw, 0, width, little); }
            }
            if (metadata && length != 0 && (tag == 700 || tag == 34675 || tag == 33723)) {
                auto packet = directory.window(value_at, length, directory.group + "." + key);
                if (tag == 700) { inspect_xmp(packet); }
                else if (tag == 34675) { inspect_icc(packet); }
                else { inspect_iptc(packet); }
            }
            if (metadata && tag == 37500 && length >= 18 && starts(raw, std::string_view("Nikon\0\2\0\0\0", 10))) {
                auto note = directory.window(value_at + 10, length - 10, directory.group + ".maker_note.nikon");
                inspect_tiff(note, true, item.depth + 1);
            }
            directory.put(key, kind, std::move(value), value_at, std::move(raw), encoding, tag, true);
        }
        if (metadata && thumbnail_length != 0) {
            directory.put("thumbnail_jpeg", ValueKind::Bytes, "", thumbnail_offset, directory.read(thumbnail_offset, thumbnail_length));
        }
        const auto next = reader.read(offset + count_width + table_bytes, inline_width);
        offset = integer(next, 0, inline_width, little);
        ++directories;
        if (offset != 0) {
            if (!reader.context.budget().reserve_allocation(512)) { throw Failure{ParseStatus::Internal, "TIFF directory allocation", reader.base}; }
            pending.push_back({offset, reader.group + ".ifd" + std::to_string(directories), item.depth});
        }
    }
    reader.derived("directory_count", directories);
}

}
