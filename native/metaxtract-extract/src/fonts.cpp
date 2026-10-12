#include "fonts.hpp"
#include "text.hpp"
#include "xml.hpp"

namespace metaxtract::native::formats {
namespace {
std::uint64_t font_num(Reader& r, std::uint64_t at, unsigned width) { return integer(r.read(at, width), 0, width); }
void font_raw(Reader& r, const std::string& key, std::uint64_t at, std::uint64_t size) { r.put(key, ValueKind::Bytes, "", at, r.read(at, size)); }
void font_table(Reader& r, const std::string& tag) {
    if (tag == "name") {
        r.require(0, 6); const auto format = font_num(r, 0, 2), count = font_num(r, 2, 2), strings = font_num(r, 4, 2);
        if (format > 1) { r.diagnostic("unsupported_name_format", "Unknown name table version retained", 0); return; }
        r.require(6, count * 12);
        if (strings < 6 + count * 12) { throw Failure{ParseStatus::Malformed, "Font strings overlap name records", r.base}; }
        for (std::uint64_t i = 0; i < count; ++i) {
            r.entry(); auto entry = r.window(6 + i * 12, 12, r.group + ".name" + std::to_string(i));
            const auto platform = font_num(entry, 0, 2), encoding = font_num(entry, 2, 2), id = font_num(entry, 6, 2);
            entry.number("platform", 0, 2); entry.number("encoding", 2, 2); entry.number("language", 4, 2); entry.number("name_id", 6, 2);
            const auto at = strings + font_num(entry, 10, 2), size = font_num(entry, 8, 2); auto bytes = r.read(at, size);
            const std::string charset = platform == 0 || (platform == 3 && (encoding == 0 || encoding == 1 || encoding == 10)) ? "UTF-16BE" : "unspecified";
            const auto text = decode_text(r, bytes, charset); auto value = r; value.group = entry.group;
            value.put("name_value", text.supported && text.valid ? ValueKind::Utf8Text : ValueKind::Bytes, text.value, at, std::move(bytes), charset, id, true);
        }
        if (format == 1) {
            const auto at = 6 + count * 12, languages = font_num(r, at, 2); r.require(at + 2, languages * 4);
            if (strings < at + 2 + languages * 4) { throw Failure{ParseStatus::Malformed, "Font strings overlap language records", r.base}; }
            for (std::uint64_t i = 0; i < languages; ++i) { r.entry(); const auto record = at + 2 + i * 4; font_raw(r, "language_tag_utf16be", strings + font_num(r, record + 2, 2), font_num(r, record, 2)); }
        }
    } else if (tag == "head") {
        r.require(0, 54); r.number("version_raw", 0, 4); r.number("font_revision_raw", 4, 4); r.number("flags", 16, 2);
        r.number("units_per_em", 18, 2); r.number("created_raw", 20, 8); r.number("modified_raw", 28, 8); font_raw(r, "bounds_signed_raw", 36, 8);
        r.number("style", 44, 2); r.number("index_to_location_format", 50, 2);
    } else if (tag == "OS/2") {
        r.require(0, 10); r.number("version", 0, 2); r.number("weight_class", 4, 2); r.number("width_class", 6, 2); r.number("embedding_permissions", 8, 2);
        if (r.size >= 68) { font_raw(r, "vendor_id", 58, 4); r.number("selection_flags", 62, 2); r.number("first_character", 64, 2); r.number("last_character", 66, 2); }
    } else if (tag == "post") { r.require(0, 16); r.number("version_raw", 0, 4); font_raw(r, "italic_angle_fixed_raw", 4, 4); r.number("fixed_pitch", 12, 4); }
}
void sfnt(Reader& r, std::uint32_t depth) {
    if (!r.context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "Font collection depth limit", r.base}; }
    const auto signature = r.read(0, 4);
    if (starts(signature, "ttcf")) {
        r.number("collection_version", 4, 4); const auto count = font_num(r, 8, 4); r.require(12, count * 4);
        for (std::uint64_t i = 0; i < count; ++i) {
            r.entry(); const auto at = font_num(r, 12 + i * 4, 4);
            // TTC table offsets refer to the whole collection, not the face header.
            r.require(at, 12); const auto tables = font_num(r, at + 4, 2); r.require(at + 12, tables * 16);
            for (std::uint64_t j = 0; j < tables; ++j) {
                r.entry(); auto entry = r.window(at + 12 + j * 16, 16, "font.face" + std::to_string(i) + ".table" + std::to_string(j));
                const auto tag_bytes = entry.read(0, 4); const std::string tag(tag_bytes.begin(), tag_bytes.end()); font_raw(entry, "tag", 0, 4); entry.number("checksum", 4, 4);
                auto table = r.window(font_num(entry, 8, 4), font_num(entry, 12, 4), entry.group); font_raw(table, "table_raw", 0, table.size); font_table(table, tag);
            }
        } return;
    }
    if (!starts(signature, "OTTO") && !starts(signature, std::string_view("\0\x01\0\0", 4)) && !starts(signature, "true") && !starts(signature, "typ1")) { throw Failure{ParseStatus::Unsupported, "Unsupported font signature", 0}; }
    const auto count = font_num(r, 4, 2); r.require(12, count * 16);
    for (std::uint64_t i = 0; i < count; ++i) {
        r.entry(); auto entry = r.window(12 + i * 16, 16, "font.table" + std::to_string(i));
        const auto tag_bytes = entry.read(0, 4); const std::string tag(tag_bytes.begin(), tag_bytes.end()); font_raw(entry, "tag", 0, 4); entry.number("checksum", 4, 4);
        const auto at = font_num(entry, 8, 4), length = font_num(entry, 12, 4); entry.number("offset", 8, 4); entry.number("length", 12, 4);
        auto table = r.window(at, length, entry.group); font_raw(table, "table_raw", 0, length); font_table(table, tag);
    }
}
}
ParseStatus inspect_font(Reader& r) {
    const auto signature = r.read(0, 4);
    if (!starts(signature, "wOFF") && !starts(signature, "wOF2")) { sfnt(r, 0); return ParseStatus::Complete; }
    const bool woff2 = starts(signature, "wOF2"); r.require(0, woff2 ? 48 : 44);
    r.number("flavor", 4, 4); r.number("declared_size", 8, 4); r.number("table_count", 12, 2); r.number("sfnt_size", 16, 4);
    if (font_num(r, 8, 4) != r.size || font_num(r, 14, 2)) { throw Failure{ParseStatus::Malformed, "Invalid WOFF header", 0}; }
    r.number("major_version", woff2 ? 24 : 20, 2); r.number("minor_version", woff2 ? 26 : 22, 2);
    const auto metadata_offset = font_num(r, woff2 ? 28 : 24, 4), metadata_size = font_num(r, woff2 ? 32 : 28, 4), metadata_decoded = font_num(r, woff2 ? 36 : 32, 4);
    const auto private_offset = font_num(r, woff2 ? 40 : 36, 4), private_size = font_num(r, woff2 ? 44 : 40, 4);
    if (private_size) { font_raw(r, "private_data", private_offset, private_size); }
    if (metadata_size) {
        auto bytes = r.read(metadata_offset, metadata_size); r.put("compressed_metadata", ValueKind::Bytes, "", metadata_offset, bytes);
        if (!woff2) { auto decoded = inflate_metadata(r, bytes, 15, metadata_decoded); if (decoded.size() != metadata_decoded) { throw Failure{ParseStatus::Malformed, "WOFF metadata size mismatch", metadata_offset}; } inspect_xml_fields(r, decoded, "font.woff.metadata"); }
    }
    if (woff2) { font_raw(r, "transformed_font_raw", 48, r.size - 48); r.diagnostic("woff2_transform_unsupported", "Brotli/transformed tables retained without semantic decoding", 48); return ParseStatus::Unsupported; }
    const auto count = font_num(r, 12, 2); r.require(44, count * 20);
    for (std::uint64_t i = 0; i < count; ++i) {
        r.entry(); auto entry = r.window(44 + i * 20, 20, "font.woff.table" + std::to_string(i)); const auto tag_bytes = entry.read(0, 4); const std::string tag(tag_bytes.begin(), tag_bytes.end());
        font_raw(entry, "tag", 0, 4); entry.number("checksum", 16, 4); const auto at = font_num(entry, 4, 4), compressed = font_num(entry, 8, 4), length = font_num(entry, 12, 4);
        if (compressed > length) { throw Failure{ParseStatus::Malformed, "WOFF table compression length is invalid", at}; }
        auto bytes = r.read(at, compressed); r.put("table_encoded", ValueKind::Bytes, "", at, bytes);
        if (compressed < length) { bytes = inflate_metadata(r, bytes, 15, length); }
        if (bytes.size() != length) { throw Failure{ParseStatus::Malformed, "WOFF table decoded length mismatch", at}; }
        const auto packet = entry.group + "/" + path_component(tag); Reader table(r.context, bytes, entry.group, packet); font_raw(table, "table_raw", 0, bytes.size()); font_table(table, tag);
    }
    return ParseStatus::Complete;
}
}
