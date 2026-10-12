#include "format.hpp"
#include "xml.hpp"

#include <pugixml.hpp>
#include <zlib.h>

#include <array>
#include <cstdlib>
#include <memory>

namespace metaxtract::native::formats {
namespace {

voidpf z_allocate(voidpf owner, const uInt items, const uInt size) {
    auto& budget = *static_cast<ParserBudget*>(owner);
    const auto length = static_cast<std::uint64_t>(items) * size;
    if (length > std::numeric_limits<std::size_t>::max() || !budget.reserve_allocation(length)) { return nullptr; }
    return std::calloc(items, size);
}

void z_free(voidpf, voidpf data) { std::free(data); }

std::size_t zero(const std::span<const std::uint8_t> bytes, const std::size_t at) {
    const auto found = std::find(bytes.begin() + static_cast<std::ptrdiff_t>(at), bytes.end(), 0);
    if (found == bytes.end()) { throw Failure{ParseStatus::Malformed, "Metadata string terminator missing", 0}; }
    return static_cast<std::size_t>(found - bytes.begin());
}

std::string text(const std::span<const std::uint8_t> bytes) {
    return {bytes.begin(), bytes.end()};
}

void icc_data(Reader& reader, const std::span<const std::uint8_t> data, const bool located) {
    const auto field = [&](const std::string& key, const std::size_t at, const std::size_t length, const ValueKind kind, const std::string& value) {
        if (at > data.size() || length > data.size() - at) { throw Failure{ParseStatus::Malformed, "ICC field outside profile", reader.base}; }
        if (located) { reader.put(key, kind, value, at, {data.begin() + static_cast<std::ptrdiff_t>(at), data.begin() + static_cast<std::ptrdiff_t>(at + length)}, "ASCII"); }
        else { reader.decoded(key, kind == ValueKind::Bytes ? ValueKind::DecodedBytes : kind,
            kind == ValueKind::Bytes ? text(data.subspan(at, length)) : value, reader.group, 3, "ASCII"); }
    };
    if (data.size() < 132 || !starts(data, "acsp", 36)) { throw Failure{ParseStatus::Malformed, "Invalid ICC header", reader.base}; }
    const auto extent = integer(data, 0, 4), count = integer(data, 128, 4);
    if (extent < 132 || extent > data.size() || count > (extent - 132) / 12) { throw Failure{ParseStatus::Malformed, "ICC size or tag table invalid", reader.base}; }
    field("profile_size", 0, 4, ValueKind::Unsigned, std::to_string(extent));
    field("cmm", 4, 4, ValueKind::Text, text(data.subspan(4, 4)));
    field("version", 8, 4, ValueKind::Bytes, "");
    field("device_class", 12, 4, ValueKind::Text, text(data.subspan(12, 4)));
    field("color_space", 16, 4, ValueKind::Text, text(data.subspan(16, 4)));
    field("connection_space", 20, 4, ValueKind::Text, text(data.subspan(20, 4)));
    field("created", 24, 12, ValueKind::Bytes, "");
    field("platform", 40, 4, ValueKind::Text, text(data.subspan(40, 4)));
    field("flags", 44, 4, ValueKind::Unsigned, std::to_string(integer(data, 44, 4)));
    field("manufacturer", 48, 4, ValueKind::Text, text(data.subspan(48, 4)));
    field("model", 52, 4, ValueKind::Text, text(data.subspan(52, 4)));
    field("rendering_intent", 64, 4, ValueKind::Unsigned, std::to_string(integer(data, 64, 4)));
    field("profile_id", 84, 16, ValueKind::Bytes, "");
    for (std::uint64_t index = 0; index < count; ++index) {
        reader.entry();
        const auto at = static_cast<std::size_t>(132 + index * 12);
        const auto offset = integer(data, at + 4, 4), length = integer(data, at + 8, 4);
        if (offset < 132 + count * 12 || length < 8 || offset > extent || length > extent - offset) { throw Failure{ParseStatus::Malformed, "ICC tag escapes profile", reader.base + at}; }
        const auto key = text(data.subspan(at, 4));
        field(key, static_cast<std::size_t>(offset), static_cast<std::size_t>(length), ValueKind::Bytes, "");
        if (starts(data, "text", static_cast<std::size_t>(offset))) {
            auto value = text(data.subspan(static_cast<std::size_t>(offset + 8), static_cast<std::size_t>(length - 8)));
            while (!value.empty() && value.back() == '\0') { value.pop_back(); }
            reader.decoded(key + ".text", ValueKind::Text, value, reader.group, 3, "ASCII");
        }
    }
}

}

std::vector<std::uint8_t> inflate_metadata(Reader& reader, const std::span<const std::uint8_t> bytes,
                                        const int window_bits, const std::uint64_t maximum_output) {
    z_stream stream{};
    stream.zalloc = z_allocate; stream.zfree = z_free; stream.opaque = &reader.context.budget();
    if (inflateInit2(&stream, window_bits) != Z_OK) { throw Failure{ParseStatus::Internal, "Could not initialize bounded inflater", reader.base}; }
    const auto cleanup = std::unique_ptr<z_stream, decltype([](z_stream* value) { inflateEnd(value); })>(&stream);
    std::array<std::uint8_t, 16384> output{};
    std::vector<std::uint8_t> result;
    std::size_t fed = 0;
    while (true) {
        reader.checkpoint();
        if (stream.avail_in == 0 && fed < bytes.size()) {
            const auto count = std::min<std::size_t>(65536, bytes.size() - fed);
            stream.next_in = const_cast<Bytef*>(bytes.data() + fed); stream.avail_in = static_cast<uInt>(count); fed += count;
        }
        stream.next_out = output.data(); stream.avail_out = static_cast<uInt>(output.size());
        const auto status = inflate(&stream, Z_NO_FLUSH);
        const auto produced = output.size() - stream.avail_out;
        if (result.size() > maximum_output || produced > maximum_output - result.size()) {
            throw Failure{ParseStatus::Malformed, "Inflated metadata exceeds its declared size", reader.base};
        }
        if (!reader.context.budget().consume_decoded(produced) || !reader.context.budget().reserve_allocation(produced * 2)) { throw Failure{ParseStatus::Internal, "Inflated metadata exceeds budget", reader.base}; }
        result.insert(result.end(), output.begin(), output.begin() + static_cast<std::ptrdiff_t>(produced));
        if (status == Z_STREAM_END) {
            if (stream.avail_in != 0 || fed != bytes.size()) { throw Failure{ParseStatus::Malformed, "Trailing bytes in compressed metadata", reader.base}; }
            return result;
        }
        if (status != Z_OK || (produced == 0 && stream.avail_in == 0 && fed == bytes.size())) { throw Failure{ParseStatus::Malformed, "Invalid or incomplete compressed metadata", reader.base}; }
    }
}

void inspect_xmp_data(Reader& reader, const std::span<const std::uint8_t> bytes, const std::string& packet) {
    XmlBudgetScope scope(reader.context.budget());
    pugi::xml_document document;
    const auto parsed = document.load_buffer(bytes.data(), bytes.size(), pugi::parse_default | pugi::parse_doctype, pugi::encoding_auto);
    if (!parsed) { throw Failure{ParseStatus::Malformed, "Invalid XMP XML: " + std::string(parsed.description()), reader.base}; }
    auto node = document.first_child();
    std::uint32_t depth = 1;
    std::uint64_t sequence = 0;
    while (node) {
        reader.entry();
        if (!reader.context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "XMP depth limit exceeded", reader.base}; }
        Reader field(reader.context, reader.base, reader.size, reader.group + ".node" + std::to_string(sequence++));
        if (node.type() == pugi::node_doctype) { reader.diagnostic("doctype", "XMP contains a DTD; custom and external entities are not expanded", 0); }
        if (node.type() == pugi::node_element) {
            field.decoded("element", ValueKind::Text, expanded_xml_name(node, node.name()), packet);
            for (const auto attribute : node.attributes()) {
                reader.entry();
                field.decoded(expanded_xml_name(node, attribute.name(), true), ValueKind::Text, attribute.value(), packet);
            }
            for (const auto child : node.children()) {
                if (child.type() == pugi::node_pcdata || child.type() == pugi::node_cdata) {
                    field.decoded(expanded_xml_name(node, node.name()), ValueKind::Text, child.value(), packet);
                }
            }
        }
        if (node.first_child()) { node = node.first_child(); ++depth; }
        else {
            while (node && !node.next_sibling()) { node = node.parent(); if (depth > 0) { --depth; } }
            if (node) { node = node.next_sibling(); }
        }
    }
}

void inspect_xmp(Reader& reader) {
    auto bytes = reader.read(0, reader.size);
    reader.put("packet", ValueKind::Bytes, "", 0, bytes);
    inspect_xmp_data(reader, bytes, reader.group);
}

void inspect_icc(Reader& reader) {
    auto bytes = reader.read(0, reader.size);
    reader.put("packet", ValueKind::Bytes, "", 0, bytes);
    icc_data(reader, bytes, true);
}

void inspect_icc_data(Reader& reader, const std::span<const std::uint8_t> bytes) {
    reader.decoded("assembled_profile", ValueKind::DecodedBytes, text(bytes), reader.group);
    icc_data(reader, bytes, false);
}

void inspect_iptc(Reader& reader) {
    std::uint64_t at = 0;
    while (at < reader.size) {
        reader.entry();
        const auto header = reader.read(at, 5);
        if (header[0] != 0x1c) { throw Failure{ParseStatus::Malformed, "Invalid IPTC dataset marker", reader.base + at}; }
        auto length = integer(header, 3, 2);
        const auto record = header[1], dataset = header[2];
        at += 5;
        if ((length & 0x8000U) != 0) {
            const auto width = length & 0x7fffU;
            if (width == 0 || width > 8) { throw Failure{ParseStatus::Malformed, "Invalid IPTC extended length", reader.base + at}; }
            length = integer(reader.read(at, width), 0, static_cast<unsigned>(width)); at += width;
        }
        auto data = reader.read(at, length);
        reader.put(std::to_string(record) + ":" + std::to_string(dataset), ValueKind::Bytes, "", at, data, "", (static_cast<std::uint64_t>(record) << 8U) | dataset, true);
        if (record == 2) { reader.decoded(std::to_string(record) + ":" + std::to_string(dataset) + ".text", ValueKind::Text, text(data), reader.group, 3, "unspecified"); }
        at += length;
    }
}

void jpeg_metadata(Reader& reader, const std::uint8_t code, const std::uint64_t at, const std::uint64_t length) {
    Reader body(reader.context, reader.base + at, length, reader.group + ".metadata");
    const auto prefix = body.read(0, std::min<std::uint64_t>(length, 80));
    if (code == 0xfe) { body.put("comment", ValueKind::Text, text(body.read(0, length)), 0, body.read(0, length), "unspecified"); }
    else if (code == 0xe1 && starts(prefix, std::string_view("Exif\0\0", 6))) {
        Reader exif(body.context, body.base + 6, length - 6, body.group + ".exif"); inspect_tiff(exif, true);
    } else if (code == 0xe1 && starts(prefix, std::string_view("http://ns.adobe.com/xap/1.0/\0", 29))) {
        Reader xmp(body.context, body.base + 29, length - 29, body.group + ".xmp"); inspect_xmp(xmp);
    } else if (code == 0xe1 && starts(prefix, std::string_view("http://ns.adobe.com/xmp/extension/\0", 35))) {
        body.put("extended_xmp", ValueKind::Bytes, "", 0, body.read(0, length));
        body.diagnostic("fragmented_xmp", "Extended XMP fragment retained; assembly is not yet supported", 0);
    } else if (code == 0xe2 && starts(prefix, std::string_view("ICC_PROFILE\0", 12))) {
        if (length < 14 || prefix[12] == 0 || prefix[13] == 0 || prefix[12] > prefix[13]) { throw Failure{ParseStatus::Malformed, "Invalid JPEG ICC sequence", body.base}; }
        body.number("icc_sequence", 12, 1); body.number("icc_part_count", 13, 1);
        body.put("icc_part", ValueKind::Bytes, "", 14, body.read(14, length - 14));
        if (prefix[13] == 1) { Reader icc(body.context, body.base + 14, length - 14, body.group + ".icc"); inspect_icc(icc); }
    } else if (code == 0xe0 && starts(prefix, std::string_view("JFIF\0", 5))) {
        if (length < 14) { throw Failure{ParseStatus::Malformed, "Incomplete JFIF header", body.base}; }
        body.number("jfif_version", 5, 2); body.number("density_unit", 7, 1);
        body.number("x_density", 8, 2); body.number("y_density", 10, 2);
        body.number("thumbnail_width", 12, 1); body.number("thumbnail_height", 13, 1);
        const auto thumbnail = static_cast<std::uint64_t>(prefix[12]) * prefix[13] * 3;
        if (thumbnail != 0) { body.put("thumbnail_rgb", ValueKind::Bytes, "", 14, body.read(14, thumbnail)); }
    } else if (code == 0xed && starts(prefix, std::string_view("Photoshop 3.0\0", 14))) {
        std::uint64_t position = 14;
        while (position < length) {
            body.entry();
            const auto header = body.read(position, 7);
            if (!starts(header, "8BIM")) { throw Failure{ParseStatus::Malformed, "Invalid Photoshop resource signature", body.base + position}; }
            const auto id = integer(header, 4, 2);
            const auto name_length = header[6];
            const auto name_bytes = (name_length + 2) & ~1ULL;
            const auto size_at = position + 6 + name_bytes;
            const auto bytes = integer(body.read(size_at, 4), 0, 4);
            Reader resource(body.context, body.base + size_at + 4, bytes, body.group + ".resource" + std::to_string(id));
            body.require(size_at + 4, bytes + bytes % 2);
            if (id == 0x0404) { inspect_iptc(resource); }
            else if (id == 0x040f) { inspect_icc(resource); }
            else if (id == 0x0424) { inspect_xmp(resource); }
            else { resource.put("payload", ValueKind::Bytes, "", 0, resource.read(0, bytes)); }
            position = size_at + 4 + bytes + bytes % 2;
        }
    } else if (code >= 0xe0 && code <= 0xef) { body.put("unknown_application_payload", ValueKind::Bytes, "", 0, body.read(0, length)); }
}

void png_metadata(Reader& reader, const std::string& type, const std::uint64_t at, const std::uint64_t length) {
    Reader body(reader.context, reader.base + at, length, reader.group + "." + type);
    if (type == "eXIf") { inspect_tiff(body, true); }
    else if (type == "tEXt" || type == "zTXt" || type == "iTXt" || type == "iCCP") {
        auto bytes = body.read(0, length);
        body.put("packet", ValueKind::Bytes, "", 0, bytes);
        const auto split = zero(bytes, 0);
        if (split == 0 || split > 79) { throw Failure{ParseStatus::Malformed, "Invalid PNG metadata keyword", body.base}; }
        body.put("keyword", ValueKind::Text, text(std::span(bytes).first(split)), 0, {bytes.begin(), bytes.begin() + static_cast<std::ptrdiff_t>(split)}, "ISO-8859-1");
        std::size_t begin = split + 1;
        bool compressed = false;
        std::string encoding = "ISO-8859-1";
        if (type == "zTXt" || type == "iCCP") {
            if (begin >= bytes.size() || bytes[begin++] != 0) { throw Failure{ParseStatus::Malformed, "Invalid PNG metadata compression", body.base}; }
            compressed = true;
        } else if (type == "iTXt") {
            if (bytes.size() - begin < 2 || bytes[begin] > 1 || bytes[begin + 1] != 0) { throw Failure{ParseStatus::Malformed, "Invalid PNG international text flags", body.base}; }
            compressed = bytes[begin] == 1; begin += 2;
            const auto language_end = zero(bytes, begin);
            body.decoded("language", ValueKind::Text, text(std::span(bytes).subspan(begin, language_end - begin)), body.group); begin = language_end + 1;
            const auto translated_end = zero(bytes, begin);
            body.decoded("translated_keyword", ValueKind::Text, text(std::span(bytes).subspan(begin, translated_end - begin)), body.group); begin = translated_end + 1;
            encoding = "UTF-8";
        }
        auto payload = std::span<const std::uint8_t>(bytes).subspan(begin);
        std::vector<std::uint8_t> inflated;
        if (compressed) { inflated = inflate_metadata(body, payload); payload = inflated; }
        if (type == "iCCP") { body.decoded("profile", ValueKind::DecodedBytes, text(payload), body.group); icc_data(body, payload, false); }
        else {
            const auto key = text(std::span(bytes).first(split));
            if (compressed) { body.decoded("text", ValueKind::Text, text(payload), body.group, 3, encoding); }
            else { body.put("text", ValueKind::Text, text(payload), begin, {payload.begin(), payload.end()}, encoding); }
            if (key == "XML:com.adobe.xmp") { inspect_xmp_data(body, payload, body.group); }
        }
    } else if (type == "pHYs") {
        if (length != 9) { throw Failure{ParseStatus::Malformed, "Invalid PNG physical dimensions", body.base}; }
        body.number("x_pixels_per_unit", 0, 4); body.number("y_pixels_per_unit", 4, 4); body.number("unit", 8, 1);
    } else if (type == "tIME") {
        if (length != 7) { throw Failure{ParseStatus::Malformed, "Invalid PNG modification time", body.base}; }
        body.number("year", 0, 2); body.number("month", 2, 1); body.number("day", 3, 1);
        body.number("hour", 4, 1); body.number("minute", 5, 1); body.number("second", 6, 1);
    } else if (type == "gAMA") { if (length != 4) { throw Failure{ParseStatus::Malformed, "Invalid PNG gamma", body.base}; } body.number("gamma_times_100000", 0, 4); }
    else if (type == "sRGB") { if (length != 1) { throw Failure{ParseStatus::Malformed, "Invalid PNG sRGB intent", body.base}; } body.number("rendering_intent", 0, 1); }
    else if (type != "IHDR" && type != "IDAT" && type != "IEND" && type != "PLTE") { body.put("payload", ValueKind::Bytes, "", 0, body.read(0, length)); }
}

void webp_metadata(Reader& reader, const std::string& type, const std::uint64_t at, const std::uint64_t length) {
    Reader body(reader.context, reader.base + at, length, reader.group + "." + type);
    if (type == "EXIF") {
        const auto signature = body.read(0, std::min<std::uint64_t>(6, length));
        const auto skip = starts(signature, std::string_view("Exif\0\0", 6)) ? 6U : 0U;
        Reader exif(body.context, body.base + skip, body.size - skip, body.group); inspect_tiff(exif, true);
    } else if (type == "XMP ") { inspect_xmp(body); }
    else if (type == "ICCP") { inspect_icc(body); }
    else if (type == "ANIM") {
        if (length != 6) { throw Failure{ParseStatus::Malformed, "Invalid WebP animation header", body.base}; }
        body.put("background_rgba", ValueKind::Bytes, "", 0, body.read(0, 4)); body.number("loop_count", 4, 2, true);
    } else if (type != "VP8X" && type != "VP8 " && type != "VP8L" && type != "ANMF" && type != "ALPH") { body.put("payload", ValueKind::Bytes, "", 0, body.read(0, length)); }
}

void gif_metadata(Reader& reader, const std::uint8_t label, const std::uint64_t start, const std::uint64_t end) {
    if (label == 0xf9) { return; }
    reader.put("encoded_subblocks", ValueKind::Bytes, "", start, reader.read(start, end - start));
    std::uint64_t at = start;
    std::string payload;
    while (at < end) {
        reader.checkpoint();
        const auto size = reader.read(at++, 1)[0];
        if (size == 0) { break; }
        if (!reader.context.budget().reserve_allocation(size * 2ULL)) { throw Failure{ParseStatus::Internal, "GIF metadata allocation", reader.base + at}; }
        payload += text(reader.read(at, size)); at += size;
    }
    reader.decoded(label == 0xfe ? "comment" : "application_data", label == 0xfe ? ValueKind::Text : ValueKind::DecodedBytes, payload, reader.group, 3, "unspecified");
    if (label == 0xff && payload.size() >= 14 && (payload.starts_with("NETSCAPE2.0") || payload.starts_with("ANIMEXTS1.0")) && payload[11] == 1) {
        reader.decoded("loop_count", ValueKind::Unsigned, std::to_string(static_cast<unsigned char>(payload[12]) | (static_cast<unsigned char>(payload[13]) << 8U)), reader.group);
    }
}

}
