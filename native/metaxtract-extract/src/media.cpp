#include "format.hpp"
#include "boxes.hpp"
#include "media_details.hpp"

#include <bit>
#include <cmath>
#include <map>
#include <set>
#include <memory>

namespace metaxtract::native::formats {
namespace {

void string_field(Reader& r, const std::string& key, std::uint64_t at, std::uint64_t length, const std::string& encoding = "UTF-8") {
    auto bytes = r.read(at, length);
    auto size = bytes.size(); while (size && bytes[size - 1] == 0) { --size; }
    std::string value(reinterpret_cast<const char*>(bytes.data()), size);
    r.put(key, ValueKind::Text, std::move(value), at, std::move(bytes), encoding);
}

void depth(Reader& reader, std::uint32_t level) {
    if (!reader.context.budget().check_depth(level)) { throw Failure{ParseStatus::Internal, "Media container depth exceeded", reader.base}; }
}

void bmff(Reader& reader, std::uint64_t begin, std::uint64_t end, std::uint32_t level, bool detailed) {
    depth(reader, level);
    unsigned index = 0;
    walk_boxes(reader, begin, end, [&](const Box& box) {
        auto r = reader.window(box.offset + box.header, box.size - box.header, reader.group + "." + path_component(box.type) + std::to_string(index++));
        reader.put("box_type", ValueKind::Text, box.type, box.offset + 4, reader.read(box.offset + 4, 4), "ISO-8859-1");
        r.derived("payload_offset", r.base); r.derived("payload_size", r.size);
        if (box.type == "ftyp") {
            r.require(0, 8); string_field(r, "major_brand", 0, 4, "ASCII"); r.number("minor_version", 4, 4);
            if ((r.size - 8) % 4) { throw Failure{ParseStatus::Malformed, "Incomplete BMFF compatible brand", r.base + 8}; }
            for (std::uint64_t at = 8; at < r.size; at += 4) { r.entry(); string_field(r, "compatible_brand", at, 4, "ASCII"); }
        } else if (box.type == "mvhd" || box.type == "mdhd") {
            const auto version = r.read(0, 1)[0]; if (version > 1) { throw Failure{ParseStatus::Unsupported, "Unknown media time header version", r.base}; }
            r.number("version", 0, 1);
            const auto width = version ? 8U : 4U;
            r.number("created_1904", 4, width); r.number("modified_1904", 4 + width, width);
            r.number("timescale", 4 + 2 * width, 4); r.number("duration_ticks", 8 + 2 * width, width);
            if (box.type == "mdhd") {
                const auto at = 8 + 3 * width; const auto data = r.read(at, 2); const auto language = integer(data, 0, 2);
                std::string value; for (int shift : {10, 5, 0}) { value.push_back(static_cast<char>(((language >> shift) & 31U) + 0x60U)); }
                r.put("language", ValueKind::Text, value, at, data, "ASCII");
            }
        } else if (box.type == "tkhd") {
            const auto version = r.read(0, 1)[0]; if (version > 1) { throw Failure{ParseStatus::Unsupported, "Unknown track header version", r.base}; }
            const auto width = version ? 8U : 4U; r.number("track_flags", 1, 3); r.number("track_id", 4 + 2 * width, 4);
            r.number("duration_ticks", 12 + 2 * width, width);
            const auto geometry = version ? 88U : 76U;
            for (const auto& [key, at] : std::initializer_list<std::pair<const char*, unsigned>>{{"display_width", geometry}, {"display_height", geometry + 4}}) {
                auto data = r.read(at, 4); const auto value = static_cast<double>(integer(data, 0, 4)) / 65536.0;
                r.put(key, ValueKind::Float, std::to_string(value), at, std::move(data));
            }
            r.put("display_matrix", ValueKind::Bytes, {}, geometry - 36, r.read(geometry - 36, 36));
        } else if (box.type == "hdlr") {
            r.require(0, 24); string_field(r, "handler_type", 8, 4, "ASCII"); if (r.size > 24) { string_field(r, "handler_name", 24, r.size - 24, "unspecified"); }
        } else if (box.type == "stsd") {
            r.require(0, 8); const auto count = integer(r.read(4, 4), 0, 4); r.number("sample_description_count", 4, 4);
            unsigned entries = 0;
            walk_boxes(r, 8, r.size, [&](const Box& entry) {
                auto sample = r.window(entry.offset + entry.header, entry.size - entry.header, r.group + ".sample" + std::to_string(entries++));
                r.put("codec_fourcc", ValueKind::Text, entry.type, entry.offset + 4, r.read(entry.offset + 4, 4), "ISO-8859-1");
                sample.put("sample_description", ValueKind::Bytes, {}, 0, sample.read(0, sample.size));
                if (sample.size >= 8) { sample.number("data_reference_index", 6, 2); }
                if (detailed) { inspect_media_sample(sample, entry.type, level + 1); }
            });
            if (entries != count) { throw Failure{ParseStatus::Malformed, "Sample-description count mismatch", r.base + 4}; }
        } else if (box.type == "stsz") {
            r.require(0, 12); r.number("default_sample_size", 4, 4); r.number("sample_count", 8, 4);
            const auto size = integer(r.read(4, 4), 0, 4), count = integer(r.read(8, 4), 0, 4);
            if (!size) { r.require(12, count * 4); }
        } else if (box.type == "stco" || box.type == "co64") {
            r.require(0, 8); const auto count = integer(r.read(4, 4), 0, 4); const auto width = box.type == "co64" ? 8U : 4U;
            r.require(8, count * width); r.number("chunk_count", 4, 4);
            for (std::uint64_t at = 8; at < 8 + count * width; at += width) { r.entry(); r.number("chunk_offset", at, width); }
        } else if (box.type == "stts" || box.type == "ctts" || box.type == "stsc" || box.type == "stss") {
            r.require(0, 8); const auto count = integer(r.read(4, 4), 0, 4);
            const auto columns = box.type == "stsc" ? 3U : box.type == "stss" ? 1U : 2U;
            r.require(8, count * columns * 4); r.number("table_entry_count", 4, 4);
            r.put("sample_table", ValueKind::Bytes, {}, 8, r.read(8, count * columns * 4));
        } else if (box.type == "tfhd") { r.number("track_flags", 1, 3); r.number("track_id", 4, 4); }
        else if (box.type == "tfdt") { const auto version = r.read(0, 1)[0]; if (version > 1) { throw Failure{ParseStatus::Unsupported, "Unknown decode-time version", r.base}; } r.number("base_decode_time", 4, version ? 8 : 4); }
        else if (box.type == "trun") { r.number("fragment_flags", 1, 3); r.number("fragment_sample_count", 4, 4); }
        else if (box.type == "moov" || box.type == "trak" || box.type == "mdia" || box.type == "minf" || box.type == "stbl" || box.type == "edts" || box.type == "udta" || box.type == "moof" || box.type == "traf" || box.type == "mvex" || box.type == "dinf" || box.type == "ilst" || (detailed && box.type == "tref")) { bmff(r, 0, r.size, level + 1, detailed); }
        else if (box.type == "meta") {
            const auto h = r.read(0, std::min<std::uint64_t>(8, r.size));
            const auto offset = h.size() >= 8 && (starts(h, "hdlr", 4) || starts(h, "keys", 4) || starts(h, "ilst", 4)) ? 0U : 4U;
            r.require(0, offset); bmff(r, offset, r.size, level + 1, detailed);
        } else if (detailed && reader.group.find(".ilst") != std::string::npos && box.type != "data") { bmff(r, 0, r.size, level + 1, detailed); }
        else if (detailed && inspect_media_box(r, box.type, level + 1)) {}
        else if (box.type != "mdat" && box.type != "free" && box.type != "skip" && box.type != "wide") {
            r.put("unknown_box", ValueKind::Bytes, {}, 0, r.read(0, r.size));
        }
    });
}

struct Vint { std::uint64_t value; unsigned width; bool unknown; };
Vint vint(Reader& reader, std::uint64_t at, bool identifier) {
    const auto first = reader.read(at, 1)[0];
    unsigned width = 1; std::uint8_t mask = 128;
    while (width <= 8 && !(first & mask)) { ++width; mask >>= 1; }
    if (width > (identifier ? 4U : 8U)) { throw Failure{ParseStatus::Malformed, "Invalid EBML variable integer", reader.base + at}; }
    const auto bytes = reader.read(at, width);
    std::uint64_t number = identifier ? first : first & (mask - 1U);
    for (unsigned n = 1; n < width; ++n) { number = (number << 8U) | bytes[n]; }
    return {number, width, !identifier && number == ((1ULL << (7U * width)) - 1)};
}

bool master(std::uint64_t id) {
    static const std::set<std::uint64_t> ids{0x1a45dfa3,0x18538067,0x1549a966,0x1654ae6b,0xae,0xe0,0xe1,0x1254c367,0x7373,0x63c0,0x67c8,0x1043a770,0x45b9,0xb6,0x80,0x1941a469,0x61a7,0x1f43b675,0xa0,0x1c53bb6b,0xbb,0xb7,0x114d9b74,0x4dbb,0x55b0,0x55d0,0x6d80,0x6240,0x5034,0x5035};
    return ids.contains(id);
}

std::uint64_t ebml(Reader& reader, std::uint64_t at, std::uint64_t end, std::uint32_t level, bool detailed, std::uint64_t parent = 0, bool unknown_parent = false) {
    depth(reader, level); unsigned index = 0;
    static const std::map<std::uint64_t, std::string> strings{{0x4282,"document_type"},{0x4d80,"muxing_app"},{0x5741,"writing_app"},{0x7ba9,"title"},{0x86,"codec_id"},{0x536e,"track_name"},{0x22b59c,"language"},{0x22b59d,"language_ietf"},{0x258688,"codec_name"},{0x466e,"attachment_name"},{0x4660,"attachment_mime"},{0x467e,"attachment_description"},{0x45a3,"tag_name"},{0x4487,"tag_value"},{0x85,"chapter_title"},{0x437c,"chapter_language"}};
    static const std::map<std::uint64_t, std::string> numbers{{0x4286,"ebml_version"},{0x4287,"doctype_version"},{0x2ad7b1,"timestamp_scale_ns"},{0xd7,"track_number"},{0x73c5,"track_uid"},{0x83,"track_type"},{0x88,"default_track"},{0xb9,"enabled_track"},{0x55aa,"forced_track"},{0x23e383,"default_duration_ns"},{0x56aa,"codec_delay_ns"},{0x56bb,"seek_preroll_ns"},{0x9f,"channels"},{0x6264,"bits_per_sample"},{0xb0,"pixel_width"},{0xba,"pixel_height"},{0x54b0,"display_width"},{0x54ba,"display_height"},{0x54b2,"display_unit"},{0x54cc,"crop_left"},{0x54dd,"crop_right"},{0x54bb,"crop_top"},{0x54aa,"crop_bottom"},{0x55b1,"matrix_coefficients"},{0x55b9,"range"},{0x55ba,"transfer_characteristics"},{0x55bb,"primaries"},{0x73c4,"chapter_uid"},{0x91,"chapter_start_ns"},{0x92,"chapter_end_ns"},{0x46ae,"attachment_uid"},{0xe7,"cluster_timestamp"}};
    static const std::map<std::uint64_t, std::string> binaries{{0x73a4,"segment_uid"},{0x63a2,"codec_private"},{0x465c,"attachment_data"},{0x4485,"tag_binary"},{0x53ab,"seek_id"}};
    while (at < end) {
        reader.entry(); const auto id = vint(reader, at, true);
        const bool sibling = id.value == 0x1f43b675 || id.value == 0x1549a966 || id.value == 0x1654ae6b || id.value == 0x1254c367 || id.value == 0x1043a770 || id.value == 0x1941a469 || id.value == 0x1c53bb6b || id.value == 0x114d9b74;
        if (unknown_parent && parent == 0x1f43b675 && sibling) { return at; }
        const auto length = vint(reader, at + id.width, false);
        const auto header = id.width + length.width;
        if (header > end - at || (!length.unknown && length.value > end - at - header)) { throw Failure{ParseStatus::Malformed, "EBML element escapes parent", reader.base + at}; }
        if (length.unknown && id.value != 0x18538067 && id.value != 0x1f43b675) { throw Failure{ParseStatus::Malformed, "Unknown EBML size on a non-streaming element", reader.base + at}; }
        const auto size = length.unknown ? end - at - header : length.value;
        auto r = reader.window(at + header, size, reader.group + ".element" + std::to_string(index++));
        reader.put("element_id", ValueKind::Unsigned, std::to_string(id.value), at, reader.read(at, id.width));
        r.derived("payload_offset", r.base); r.derived(length.unknown ? "payload_size_upper_bound" : "payload_size", size);
        r.derived("declared_size_unknown", length.unknown ? 1 : 0);
        if (master(id.value)) { const auto consumed = ebml(r, 0, r.size, level + 1, detailed, id.value, length.unknown); r.derived("parsed_payload_size", consumed); at += header + consumed; }
        else {
            if (strings.contains(id.value)) { string_field(r, strings.at(id.value), 0, size); }
            else if (numbers.contains(id.value)) {
                if (!size || size > 8) { throw Failure{ParseStatus::Malformed, "Invalid EBML unsigned value size", r.base}; }
                r.number(numbers.at(id.value), 0, static_cast<unsigned>(size));
            } else if (id.value == 0x4489 || id.value == 0xb5 || id.value == 0x78b5) {
                if (size != 4 && size != 8) { throw Failure{ParseStatus::Malformed, "Invalid EBML floating value size", r.base}; }
                auto bytes = r.read(0, size); const auto bits = integer(bytes, 0, static_cast<unsigned>(size));
                const auto value = size == 4 ? static_cast<double>(std::bit_cast<float>(static_cast<std::uint32_t>(bits))) : std::bit_cast<double>(bits);
                if (!std::isfinite(value)) { throw Failure{ParseStatus::Malformed, "Non-finite EBML float", r.base}; }
                r.put(id.value == 0x4489 ? "duration_ticks" : id.value == 0xb5 ? "sample_rate" : "output_sample_rate", ValueKind::Float, std::to_string(value), 0, std::move(bytes));
            } else if (id.value == 0x4461) {
                if (size != 8) { throw Failure{ParseStatus::Malformed, "Invalid Matroska date size", r.base}; }
                auto bytes = r.read(0, 8); const auto value = std::bit_cast<std::int64_t>(integer(bytes, 0, 8));
                r.put("date_utc_ns_2001", ValueKind::Signed, std::to_string(value), 0, std::move(bytes));
            } else if (binaries.contains(id.value)) { r.put(binaries.at(id.value), ValueKind::Bytes, {}, 0, r.read(0, size)); }
            else if (detailed && (id.value == 0xa3 || id.value == 0xa1)) { inspect_ebml_block(r, id.value == 0xa3); }
            else if (id.value != 0xa3 && id.value != 0xa1 && id.value != 0xec && id.value != 0xbf) { r.put("unknown_element", ValueKind::Bytes, {}, 0, r.read(0, size)); }
            at += header + size;
        }
    }
    return at;
}

void avi(Reader& reader, std::uint64_t at, std::uint64_t end, std::uint32_t level, bool detailed, std::string list = "") {
    depth(reader, level); unsigned index = 0; std::string stream_kind;
    while (at < end) {
        reader.entry(); if (end - at < 8) { throw Failure{ParseStatus::Malformed, "Truncated AVI chunk", reader.base + at}; }
        const auto h = reader.read(at, 8); const auto size = integer(h, 4, 4, true);
        if (size > end - at - 8) { throw Failure{ParseStatus::Malformed, "AVI chunk escapes parent", reader.base + at}; }
        const std::string name(reinterpret_cast<const char*>(h.data()), 4);
        auto r = reader.window(at + 8, size, reader.group + ".chunk" + std::to_string(index++));
        reader.put("chunk_id", ValueKind::Text, name, at, reader.read(at, 4), "ASCII"); r.derived("payload_offset", r.base); r.derived("payload_size", size);
        if (name == "LIST" || name == "RIFF") {
            string_field(r, "list_type", 0, 4, "ASCII"); const auto data = r.read(0, 4); const std::string type(reinterpret_cast<const char*>(data.data()), 4);
            if (type != "movi") { avi(r, 4, size, level + 1, detailed, type); }
        } else if (name == "avih") {
            r.require(0, 40); r.number("microseconds_per_frame", 0, 4, true); r.number("max_bytes_per_second", 4, 4, true); r.number("flags", 12, 4, true);
            r.number("total_frames", 16, 4, true); r.number("stream_count", 24, 4, true); r.number("width", 32, 4, true); r.number("height", 36, 4, true);
        } else if (name == "strh") {
            r.require(0, 48); const auto kind = r.read(0, 4); stream_kind.assign(kind.begin(), kind.end()); string_field(r, "stream_type", 0, 4, "ASCII"); string_field(r, "codec_fourcc", 4, 4, "ASCII");
            r.number("stream_flags", 8, 4, true); r.number("scale", 20, 4, true); r.number("rate", 24, 4, true); r.number("start", 28, 4, true);
            r.number("length", 32, 4, true); r.number("sample_size", 44, 4, true);
        } else if (name == "strf") { r.put("format_header", ValueKind::Bytes, {}, 0, r.read(0, size)); if (detailed) { inspect_avi_format(r, stream_kind); } }
        else if (name == "dmlh") { r.number("total_frames", 0, 4, true); }
        else if (name == "idx1") { if (size % 16) { throw Failure{ParseStatus::Malformed, "Incomplete AVI index entry", r.base}; } r.derived("index_entries", size / 16); }
        else if (list == "INFO") { string_field(r, "info_text", 0, size, "unspecified"); }
        else if (name != "JUNK") { r.put("unknown_chunk", ValueKind::Bytes, {}, 0, r.read(0, size)); }
        at += 8 + size + (size & 1U);
        if (at > end) { throw Failure{ParseStatus::Malformed, "AVI padding escapes parent", reader.base + at}; }
    }
}

std::uint64_t start_code(Reader& reader, std::uint64_t at) {
    std::uint32_t prefix = 0xffffffU;
    while (at < reader.size) {
        reader.checkpoint(); const auto bytes = reader.read(at, std::min<std::uint64_t>(65536, reader.size - at));
        for (std::size_t n = 0; n < bytes.size(); ++n) {
            prefix = ((prefix << 8U) | bytes[n]) & 0xffffffU;
            if (prefix == 1 && at + n >= 2) { return at + n - 2; }
        }
        at += bytes.size();
    }
    return reader.size;
}

void mpeg(Reader& reader, bool detailed) {
    auto at = start_code(reader, 0); unsigned index = 0;
    if (at == reader.size) { throw Failure{ParseStatus::Malformed, "MPEG start code missing", reader.base}; }
    while (at < reader.size) {
        reader.entry(); const auto h = reader.read(at, 4); const auto code = h[3];
        auto r = reader.window(at, reader.size - at, reader.group + ".packet" + std::to_string(index++));
        r.number("start_code", 3, 1);
        std::uint64_t next = 0;
        if (code == 0xb3) {
            const auto data = r.read(4, 8); const auto dimensions = integer(data, 0, 3);
            r.put("width", ValueKind::Unsigned, std::to_string(dimensions >> 12U), 4, r.read(4, 2));
            r.put("height", ValueKind::Unsigned, std::to_string(dimensions & 4095U), 5, r.read(5, 2));
            r.derived("aspect_ratio_code", data[3] >> 4U); r.derived("frame_rate_code", data[3] & 15U);
            r.derived("bitrate_units_400", integer(data, 4, 3) >> 6U); next = start_code(reader, at + 12);
        } else if (code == 0xba) {
            const auto flags = r.read(4, 1)[0]; const bool mpeg2 = (flags & 0xc0U) == 0x40;
            const auto size = mpeg2 ? 14U : 12U; r.require(0, size);
            const auto padding = mpeg2 ? r.read(13, 1)[0] & 7U : 0U; r.require(size, padding); next = at + size + padding;
            r.put("pack_header", ValueKind::Bytes, {}, 0, r.read(0, size + padding));
        } else if (code >= 0xbb) {
            const auto size = integer(r.read(4, 2), 0, 2); r.derived("packet_payload_size", size);
            if (size) { r.require(6, size); next = at + 6 + size; }
            else { next = start_code(reader, at + 6); }
            if ((code >= 0xc0 && code <= 0xef) || code == 0xbd) { r.derived("stream_id", code); if (detailed && next - at >= 9) { auto packet = r.window(0, next - at, r.group); inspect_mpeg_timestamps(packet, 6); } }
            if (code == 0xbb || code == 0xbc) { r.put("system_header", ValueKind::Bytes, {}, 6, r.read(6, next - at - 6)); }
        } else if (code == 0xb2) {
            next = start_code(reader, at + 4); r.put("user_data", ValueKind::Bytes, {}, 4, r.read(4, next - at - 4));
        } else { next = start_code(reader, at + 4); }
        if (next <= at) { throw Failure{ParseStatus::Malformed, "MPEG parser did not advance", reader.base + at}; }
        at = next;
    }
}

struct PsiState { std::vector<std::uint8_t> bytes; std::uint32_t expected = 0; };
void psi(Reader& reader, std::span<const std::uint8_t> data, std::uint16_t pid, std::map<std::uint16_t, std::uint16_t>& programs, bool detailed) {
    Reader r(reader.context, data, "media.ts.pid" + std::to_string(pid) + ".psi", "ts.psi.pid" + std::to_string(pid));
    r.require(0, 8); r.number("table_id", 0, 1); r.number("table_id_extension", 3, 2); r.number("version_flags", 5, 1);
    r.number("section_number", 6, 1); r.number("last_section_number", 7, 1);
    std::uint32_t crc = 0xffffffffU;
    for (auto byte : data) {
        crc ^= static_cast<std::uint32_t>(byte) << 24U;
        for (unsigned n = 0; n < 8; ++n) { crc = (crc << 1U) ^ ((crc & 0x80000000U) ? 0x04c11db7U : 0U); }
    }
    if (crc) { throw Failure{ParseStatus::Malformed, "PSI section CRC mismatch", reader.base}; }
    const auto id = data[0];
    if (id == 0) {
        if (data.size() < 12 || (data.size() - 12) % 4) { throw Failure{ParseStatus::Malformed, "Invalid PAT section size", reader.base}; }
        for (std::size_t at = 8; at < data.size() - 4; at += 4) {
            r.entry(); const auto program = static_cast<std::uint16_t>(integer(data, at, 2)); const auto target = static_cast<std::uint16_t>(integer(data, at + 2, 2) & 8191U);
            r.number("program_number", at, 2); r.put("program_map_pid", ValueKind::Unsigned, std::to_string(target), at + 2, r.read(at + 2, 2));
            if (program) { programs[target] = program; }
        }
    } else if (id == 2) {
        r.require(0, 16); r.put("pcr_pid", ValueKind::Unsigned, std::to_string(integer(data, 8, 2) & 8191U), 8, r.read(8, 2));
        const auto program_info = integer(data, 10, 2) & 4095U;
        if (program_info > data.size() - 16) { throw Failure{ParseStatus::Malformed, "PMT program descriptors escape section", reader.base}; }
        r.put("program_descriptors", ValueKind::Bytes, {}, 12, r.read(12, program_info));
        if (detailed) { auto descriptors = r.window(12, program_info, r.group + ".program"); inspect_transport_descriptors(descriptors); }
        for (std::uint64_t at = 12 + program_info; at < data.size() - 4;) {
            r.entry(); if (data.size() - 4 - at < 5) { throw Failure{ParseStatus::Malformed, "Truncated PMT stream", reader.base}; }
            const auto length = integer(data, static_cast<std::size_t>(at + 3), 2) & 4095U;
            if (length > data.size() - 4 - at - 5) { throw Failure{ParseStatus::Malformed, "PMT descriptor escapes section", reader.base}; }
            r.number("stream_type", at, 1); r.put("elementary_pid", ValueKind::Unsigned, std::to_string(integer(data, static_cast<std::size_t>(at + 1), 2) & 8191U), at + 1, r.read(at + 1, 2));
            r.put("stream_descriptors", ValueKind::Bytes, {}, at + 5, r.read(at + 5, length));
            if (detailed) { auto descriptors = r.window(at + 5, length, r.group + ".pid" + std::to_string(integer(data, static_cast<std::size_t>(at + 1), 2) & 8191U)); inspect_transport_descriptors(descriptors); }
            at += 5 + length;
        }
    }
    r.put("section", ValueKind::Bytes, {}, 0, r.read(0, r.size));
}

void transport(Reader& reader, bool detailed) {
    const auto head = reader.read(0, std::min<std::uint64_t>(612, reader.size));
    unsigned stride = 0, prefix = 0;
    for (auto width : {188U,192U,204U}) {
        const auto skip = width == 192 ? 4U : 0U;
        if (head.size() > skip + width && head[skip] == 0x47 && head[skip + width] == 0x47) { stride = width; prefix = skip; break; }
    }
    if (!stride) { throw Failure{ParseStatus::Malformed, "Transport packet alignment not found", reader.base}; }
    std::map<std::uint16_t, PsiState> sections; std::map<std::uint16_t, std::uint16_t> programs;
    std::map<std::uint16_t, unsigned> continuity; std::map<std::uint16_t, std::uint64_t> counts;
    for (std::uint64_t at = 0; stride <= reader.size - at; at += stride) {
        reader.entry(); auto r = reader.window(at + prefix, 188, "media.ts"); const auto h = r.read(0, 4);
        if (h[0] != 0x47) { throw Failure{ParseStatus::Malformed, "Transport sync byte missing", r.base}; }
        const auto pid = static_cast<std::uint16_t>(integer(h, 1, 2) & 8191U); const auto adaptation = (h[3] >> 4U) & 3U; const auto counter = h[3] & 15U;
        if (!adaptation) { throw Failure{ParseStatus::Malformed, "Reserved transport adaptation control", r.base + 3}; }
        r.put("pid", ValueKind::Unsigned, std::to_string(pid), 1, r.read(1, 2)); r.derived("scrambling", h[3] >> 6U); ++counts[pid];
        std::uint64_t pos = 4; bool discontinuity = false;
        if (adaptation & 2U) { const auto length = r.read(4, 1)[0]; r.require(5, length); pos = 5 + length; if (length) { discontinuity = (r.read(5, 1)[0] & 128U) != 0; } }
        if (!(adaptation & 1U) || (h[3] & 0xc0U) || (h[1] & 0x80U)) { continue; }
        if (pid != 8191 && continuity.contains(pid) && !discontinuity && counter != ((continuity[pid] + 1U) & 15U) && counter != continuity[pid]) { r.diagnostic("continuity_gap", "Transport continuity counter gap", 3); sections[pid].bytes.clear(); sections[pid].expected = 0; }
        continuity[pid] = counter;
        if (pid != 0 && !programs.contains(pid)) {
            if (detailed && (h[1] & 0x40U) && pos + 9 <= r.size) {
                const auto pes = r.read(pos, 6);
                if (pes[0] == 0 && pes[1] == 0 && pes[2] == 1) { inspect_mpeg_timestamps(r, pos + 6, true); }
            }
            continue;
        }
        auto& section = sections[pid];
        if (discontinuity) { section.bytes.clear(); section.expected = 0; }
        if (!(h[1] & 0x40U) && section.bytes.empty()) { continue; }
        if (h[1] & 0x40U) {
            const auto pointer = r.read(pos++, 1)[0]; r.require(pos, pointer);
            if (!section.bytes.empty() && pointer) {
                const auto tail = r.read(pos, pointer);
                if (!reader.context.budget().reserve_allocation(2 * tail.size()) || !reader.context.budget().consume_decoded(tail.size())) { throw Failure{ParseStatus::Internal, "PSI continuation budget exceeded", r.base + pos}; }
                section.bytes.insert(section.bytes.end(), tail.begin(), tail.end());
                if (section.expected && section.bytes.size() >= section.expected) { psi(reader, std::span(section.bytes).first(section.expected), pid, programs, detailed); }
            }
            section.bytes.clear(); section.expected = 0; pos += pointer;
        }
        const auto bytes = r.read(pos, r.size - pos);
        for (auto byte : bytes) {
            if (section.bytes.empty() && byte == 255) { break; }
            if (!reader.context.budget().reserve_allocation(2) || !reader.context.budget().consume_decoded(1)) { throw Failure{ParseStatus::Internal, "PSI assembly budget exceeded", r.base + pos}; }
            section.bytes.push_back(byte);
            if (section.bytes.size() == 3) {
                section.expected = static_cast<std::uint32_t>(((section.bytes[1] & 15U) << 8U) | section.bytes[2]) + 3;
                if (section.expected < 8 || section.expected > 1024) { throw Failure{ParseStatus::Malformed, "Invalid PSI section length", r.base + pos}; }
            }
            if (section.expected && section.bytes.size() == section.expected) { psi(reader, section.bytes, pid, programs, detailed); section.bytes.clear(); section.expected = 0; }
        }
    }
    for (const auto& [pid, count] : counts) { reader.decoded("pid_packet_count", ValueKind::Text, std::to_string(pid) + ":" + std::to_string(count), "ts", 0); }
    for (const auto& [pid, state] : sections) { static_cast<void>(pid); if (!state.bytes.empty()) { reader.diagnostic("incomplete_psi", "Transport stream ends in a partial PSI section", reader.size); } }
    if (reader.size % stride) { reader.diagnostic("trailing_bytes", "Transport stream ends in a partial packet", reader.size - reader.size % stride); }
    reader.derived("packet_stride", stride);
}

class MediaParser final : public Parser {
    bool detailed_;
public:
    explicit MediaParser(bool detailed = false) : detailed_(detailed) {}
    const ParserCapability& capability() const noexcept override {
        static const ParserCapability value{"media.structure", "Bounded native media containers", "1", Extract, {"mp4","mov","matroska","webm","avi","mpeg_ps","mpeg_es","mpeg_ts"}};
        static const ParserCapability detail{"media.details", "Native media tracks chapters attachments and codec configuration", "1", Extract, value.formats};
        return detailed_ ? detail : value;
    }
    ParseStatus run(ParserContext& context) const override {
        if (context.source_size() < 4) { return ParseStatus::Unsupported; }
        Reader reader(context, 0, context.source_size(), "media");
        try {
            const auto h = reader.read(0, std::min<std::uint64_t>(16, reader.size));
            if (starts(h, "\x1a\x45\xdf\xa3")) { reader.group = "media.matroska"; ebml(reader, 0, reader.size, 0, detailed_); }
            else if (starts(h, "RIFF") && starts(h, "AVI ", 8)) {
                reader.group = "media.avi"; const auto size = integer(h, 4, 4, true); reader.require(8, size); if (size < 4) { return ParseStatus::Malformed; }
                avi(reader, 12, 8 + size, 0, detailed_);
                if (size + 8 < reader.size) { avi(reader, size + 8, reader.size, 0, detailed_); }
            } else if (h.size() >= 8 && (starts(h,"ftyp",4) || starts(h,"moov",4) || starts(h,"mdat",4) || starts(h,"wide",4) || starts(h,"free",4))) { reader.group = "media.bmff"; bmff(reader, 0, reader.size, 0, detailed_); }
            else if ((h[0] == 0 && h[1] == 0 && h[2] == 1) || (h[0] == 0 && h[1] == 0 && h[2] == 0 && h[3] == 1)) { reader.group = "media.mpeg"; mpeg(reader, detailed_); }
            else if (h[0] == 0x47 || (h.size() > 4 && h[4] == 0x47)) { reader.group = "media.ts"; transport(reader, detailed_); }
            else { return ParseStatus::Unsupported; }
            return ParseStatus::Complete;
        } catch (const Failure& failure) { reader.diagnostic("malformed", failure.message, std::min(reader.size, failure.offset)); return failure.status; }
    }
};
}
}
namespace metaxtract::native {
std::unique_ptr<Parser> media_details_parser() { return std::make_unique<formats::MediaParser>(true); }
std::unique_ptr<Parser> media_structure_parser() { return std::make_unique<formats::MediaParser>(); }
}
