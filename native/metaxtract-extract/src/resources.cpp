#include "fonts.hpp"
#include "text.hpp"
#include "xml.hpp"
#include "zip.hpp"

namespace metaxtract::native::formats {
namespace {
std::uint64_t resource_num(Reader& r, std::uint64_t at, unsigned width, bool little = false) { return integer(r.read(at, width), 0, width, little); }
void resource_raw(Reader& r, const std::string& key, std::uint64_t at, std::uint64_t length, std::uint64_t tag = 0, bool numeric = false) { r.put(key, ValueKind::Bytes, "", at, r.read(at, length), "binary", tag, numeric); }
void manifest_fields(Reader& r, std::span<const std::uint8_t> bytes, const std::string& packet) {
    std::string key, value; std::uint64_t index = 0;
    auto flush = [&] {
        if (!key.empty()) { auto field = r; field.group += ".header" + std::to_string(index++); field.decoded("name", ValueKind::Text, key, packet); field.decoded("value", ValueKind::DecodedBytes, value, packet, 3, "unspecified"); }
        key.clear(); value.clear();
    };
    for (std::size_t at = 0; at < bytes.size();) {
        r.entry(); const auto first = at; while (at < bytes.size() && bytes[at] != '\n') { ++at; }
        std::string line(bytes.begin() + static_cast<std::ptrdiff_t>(first), bytes.begin() + static_cast<std::ptrdiff_t>(at));
        if (at < bytes.size()) { ++at; } if (!line.empty() && line.back() == '\r') { line.pop_back(); }
        if (!line.empty() && (line[0] == ' ' || line[0] == '\t') && !key.empty()) { value += line.substr(1); continue; }
        flush(); const auto colon = line.find(':'); if (colon == std::string::npos) { continue; }
        key = line.substr(0, colon);
        if (!std::all_of(key.begin(), key.end(), [](unsigned char c) { return c >= 33 && c <= 126; })) { key.clear(); continue; }
        value = line.substr(colon + 1); if (!value.empty() && value[0] == ' ') { value.erase(0, 1); }
    } flush();
}

void android_xml(Reader& r) {
    r.require(0, 8); if (resource_num(r, 0, 2, true) != 3) { throw Failure{ParseStatus::Malformed, "Invalid Android XML container", 0}; }
    const auto declared = resource_num(r, 4, 4, true); if (declared != r.size) { throw Failure{ParseStatus::Malformed, "Android XML size mismatch", 0}; }
    std::vector<std::string> strings;
    std::vector<std::pair<std::uint64_t, std::uint64_t>> elements;
    auto string = [&](std::uint64_t index) -> std::string {
        if (index == 0xffffffffU) { return {}; }
        if (index >= strings.size()) { throw Failure{ParseStatus::Malformed, "Android XML string index out of range", 0}; } return strings[static_cast<std::size_t>(index)];
    };
    std::uint64_t at = resource_num(r, 2, 2, true); std::uint32_t depth = 0;
    if (at < 8) { throw Failure{ParseStatus::Malformed, "Android XML header size is invalid", 0}; }
    while (at < r.size) {
        r.entry(); r.require(at, 8); const auto type = resource_num(r, at, 2, true), header = resource_num(r, at + 2, 2, true), length = resource_num(r, at + 4, 4, true);
        if (header < 8 || length < header) { throw Failure{ParseStatus::Malformed, "Invalid Android XML chunk size", at}; }
        auto chunk = r.window(at, length, r.group + ".chunk" + std::to_string(at)); resource_raw(chunk, "chunk_raw", 0, length); chunk.number("chunk_type", 0, 2, true);
        if (type == 1) {
            if (header < 28) { throw Failure{ParseStatus::Malformed, "Android string pool header is too small", at}; }
            chunk.require(0, 28); const auto count = resource_num(chunk, 8, 4, true), styles = resource_num(chunk, 12, 4, true), flags = resource_num(chunk, 16, 4, true), start = resource_num(chunk, 20, 4, true);
            chunk.require(header, (count + styles) * 4);
            if (start < header + (count + styles) * 4 || !r.context.budget().reserve_allocation(count * sizeof(std::string))) { throw Failure{ParseStatus::Malformed, "Invalid Android XML string storage", at}; }
            strings.clear();
            for (std::uint64_t i = 0; i < count; ++i) {
                r.entry(); auto position = start + resource_num(chunk, header + i * 4, 4, true); const bool utf8 = (flags & 0x100U) != 0;
                auto encoded_length = [&](bool short_form) {
                    const auto width = short_form ? 1U : 2U, high = short_form ? 0x80U : 0x8000U; const auto first = resource_num(chunk, position, width, true); position += width;
                    if (first & high) { const auto next = resource_num(chunk, position, width, true); position += width; return ((first & (high - 1)) << (short_form ? 8 : 16)) | next; } return first;
                };
                const auto characters = encoded_length(utf8); const auto size = utf8 ? encoded_length(true) : characters * 2;
                const auto bytes = chunk.read(position, size); chunk.require(position + size, utf8 ? 1 : 2);
                if (resource_num(chunk, position + size, utf8 ? 1 : 2, true)) { throw Failure{ParseStatus::Malformed, "Android XML string is unterminated", at}; }
                const auto text = decode_text(chunk, bytes, utf8 ? "UTF-8" : "UTF-16LE");
                if (!text.valid) { throw Failure{ParseStatus::Malformed, "Android XML string encoding is invalid", at}; }
                if (!r.context.budget().reserve_allocation(text.value.size())) { throw Failure{ParseStatus::Internal, "Android XML string allocation limit", at}; }
                strings.push_back(text.value); chunk.put("string", ValueKind::Text, text.value, position, bytes, utf8 ? "UTF-8" : "UTF-16LE");
            }
        } else if (type == 0x102) {
            if (header != 16) { throw Failure{ParseStatus::Malformed, "Invalid Android element header", at}; }
            chunk.require(0, 36); ++depth; if (!r.context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "Android XML depth limit", at}; }
            if (!r.context.budget().reserve_allocation(sizeof(elements.front()))) { throw Failure{ParseStatus::Internal, "Android element stack allocation limit", at}; }
            elements.emplace_back(resource_num(chunk, 16, 4, true), resource_num(chunk, 20, 4, true));
            chunk.decoded("element", ValueKind::Text, string(resource_num(chunk, 20, 4, true)), chunk.group);
            const auto offset = resource_num(chunk, 24, 2, true), stride = resource_num(chunk, 26, 2, true), count = resource_num(chunk, 28, 2, true);
            if (offset < 20 || stride < 20) { throw Failure{ParseStatus::Malformed, "Invalid Android XML attribute table", at}; } chunk.require(16 + offset, count * stride);
            for (std::uint64_t i = 0; i < count; ++i) {
                chunk.entry(); auto field = chunk.window(16 + offset + i * stride, stride, chunk.group + ".attribute" + std::to_string(i));
                if (resource_num(field, 12, 2, true) != 8 || resource_num(field, 14, 1, true)) { throw Failure{ParseStatus::Malformed, "Invalid Android typed attribute", at}; }
                const auto name = string(resource_num(field, 4, 4, true)), uri = string(resource_num(field, 0, 4, true));
                field.decoded("name", ValueKind::Text, uri.empty() ? name : "{" + uri + "}" + name, field.group);
                field.number("type", 15, 1, true); field.number("data", 16, 4, true);
                const auto raw_index = resource_num(field, 8, 4, true), kind = resource_num(field, 15, 1, true), data = resource_num(field, 16, 4, true);
                if (raw_index != 0xffffffffU || kind == 3) { field.decoded("value", ValueKind::Text, string(raw_index != 0xffffffffU ? raw_index : data), field.group); }
            }
        } else if (type == 0x103) {
            chunk.require(0, 24);
            if (header != 16 || !depth || elements.back() != std::pair{resource_num(chunk, 16, 4, true), resource_num(chunk, 20, 4, true)}) { throw Failure{ParseStatus::Malformed, "Unbalanced Android XML element", at}; }
            elements.pop_back(); --depth;
        }
        at += length;
    }
    if (depth) { throw Failure{ParseStatus::Malformed, "Unclosed Android XML element", at}; }
}

ParseStatus resource_package(Reader& r) {
    const auto entries = zip_directory(r); bool selected = false;
    for (const auto& entry : entries) {
        r.entry(); const auto& name = entry.name;
        const bool xml = name == "META-INF/container.xml" || name.ends_with(".opf") || name.ends_with(".ncx") || name == "AndroidManifest.xml" || name == "AppxManifest.xml" || name == "manifest.xml";
        const bool headers = name == "META-INF/MANIFEST.MF" || name.ends_with(".dist-info/METADATA") || name.ends_with(".dist-info/WHEEL") || name.ends_with("PKG-INFO");
        if (!xml && !headers && name != "mimetype" && name != "resources.arsc") { continue; }
        selected = true; const auto bytes = zip_payload(r, entry); const auto packet = "resource.zip.entry" + std::to_string(entry.index) + "/" + path_component(name);
        Reader part(r.context, bytes, "resource.package.entry" + std::to_string(entry.index), packet);
        part.decoded("filename", ValueKind::DecodedBytes, name, packet, 3, "unspecified"); resource_raw(part, "part_raw", 0, bytes.size());
        if (xml) { if (bytes.size() >= 2 && integer(bytes, 0, 2, true) == 3) { android_xml(part); } else { inspect_xml_fields(part, bytes, packet); } }
        else if (headers) { manifest_fields(part, bytes, packet); }
        else if (name == "mimetype") { part.decoded("media_type", ValueKind::Text, {bytes.begin(), bytes.end()}, packet); }
    }
    return selected ? ParseStatus::Complete : ParseStatus::Unsupported;
}
void palm_book(Reader& r) {
    r.require(0, 78); resource_raw(r, "database_name_raw", 0, 32); r.number("attributes", 32, 2); r.number("version", 34, 2);
    r.number("created_raw", 36, 4); r.number("modified_raw", 40, 4); r.number("backup_raw", 44, 4); resource_raw(r, "database_type", 60, 4); resource_raw(r, "creator", 64, 4);
    const auto count = resource_num(r, 76, 2); r.require(78, count * 8); r.derived("record_count", count);
    for (std::uint64_t i = 0; i < count; ++i) { r.entry(); auto entry = r.window(78 + i * 8, 8, "ebook.palm.record" + std::to_string(i)); entry.number("offset", 0, 4); resource_raw(entry, "attributes_and_id", 4, 4); r.require(resource_num(entry, 0, 4), 0); }
    if (!count) { return; } const auto start = resource_num(r, 78, 4), end = count > 1 ? resource_num(r, 86, 4) : r.size;
    if (end < start) { throw Failure{ParseStatus::Malformed, "Palm record offsets are not ordered", start}; }
    auto header = r.window(start, end - start, "ebook.mobi"); header.require(0, 16); header.number("compression", 0, 2); header.number("text_length", 4, 4); header.number("text_record_count", 8, 2); header.number("encryption", 12, 2);
    if (header.size < 24 || !starts(header.read(16, 4), "MOBI")) { return; }
    const auto length = resource_num(header, 20, 4); if (length < 24) { throw Failure{ParseStatus::Malformed, "MOBI header is too short", start}; } header.require(16, length);
    resource_raw(header, "mobi_header_raw", 16, length); header.number("book_type", 24, 4); header.number("text_encoding", 28, 4); header.number("unique_id", 32, 4); header.number("mobi_version", 36, 4);
    const auto exth_at = 16 + length; if (exth_at + 12 > header.size || !starts(header.read(exth_at, 4), "EXTH")) { return; }
    auto exth = header.window(exth_at, resource_num(header, exth_at + 4, 4), "ebook.exth"); exth.require(0, 12); const auto records = resource_num(exth, 8, 4); std::uint64_t position = 12;
    for (std::uint64_t i = 0; i < records; ++i) {
        r.entry(); exth.require(position, 8); const auto type = resource_num(exth, position, 4), size = resource_num(exth, position + 4, 4);
        if (size < 8) { throw Failure{ParseStatus::Malformed, "MOBI EXTH record is too short", start + exth_at + position}; }
        auto field = exth.window(position, size, "ebook.exth.record" + std::to_string(i)); field.number("type", 0, 4); resource_raw(field, "value_raw", 8, size - 8, type, true);
        if (resource_num(header, 28, 4) == 65001 && (type == 100 || type == 101 || type == 103 || type == 104 || type == 105 || type == 106 || type == 108 || type == 109 || type == 112 || type == 113 || type == 503)) {
            auto bytes = field.read(8, size - 8); const auto text = decode_text(field, bytes, "UTF-8"); if (text.valid) { field.put("text", ValueKind::Text, text.value, 8, std::move(bytes), "UTF-8", type, true); }
        }
        position += size;
    }
}
}
}
namespace metaxtract::native {
namespace {
class ResourceParser final : public Parser {
    const ParserCapability& capability() const noexcept override {
        static const ParserCapability value{"formats.resource", "Font ebook and package resource metadata", "1", Extract, {"ttf", "otf", "ttc", "woff", "woff2", "epub", "mobi", "azw", "jar", "apk", "appx", "whl"}}; return value;
    }
    ParseStatus run(ParserContext& context) const override {
        formats::Reader r(context, 0, context.source_size(), "resource");
        try {
            const auto signature = r.read(0, std::min<std::uint64_t>(4, r.size));
            if (formats::starts(signature, "PK")) { return formats::resource_package(r); }
            if (r.size >= 78 && formats::starts(r.read(60, 8), "BOOKMOBI")) { formats::palm_book(r); return ParseStatus::Complete; }
            return formats::inspect_font(r);
        } catch (const formats::Failure& error) { r.diagnostic("failure", error.message, 0); return error.status; }
    }
};
}
std::unique_ptr<Parser> resource_metadata_parser() { return std::make_unique<ResourceParser>(); }
}
