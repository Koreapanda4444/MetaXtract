#include "format.hpp"
#include "text.hpp"
#include "xml.hpp"

#include <bit>
#include <charconv>
#include <cmath>
#include <map>
#include <set>

namespace metaxtract::native::formats {
namespace {
std::uint64_t gnum(Reader& r, std::uint64_t at, unsigned width, bool little = true) { return integer(r.read(at, width), 0, width, little); }
void graw(Reader& r, const std::string& key, std::uint64_t at, std::uint64_t length) { r.put(key, ValueKind::Bytes, "", at, r.read(at, length)); }
void gdouble(Reader& r, const std::string& key, std::uint64_t at, bool little = true) {
    auto bytes = r.read(at, 8); const auto value = std::bit_cast<double>(integer(bytes, 0, 8, little));
    std::ostringstream text; text << std::setprecision(17) << value;
    r.put(key, std::isfinite(value) ? ValueKind::Float : ValueKind::Bytes, text.str(), at, std::move(bytes));
    if (!std::isfinite(value)) { r.diagnostic("nonfinite_coordinate", "Nonfinite numeric declaration retained as bytes", at); }
}
void append_utf8(std::string& text, std::uint32_t code) {
    if (code < 0x80) { text.push_back(static_cast<char>(code)); }
    else if (code < 0x800) { text.push_back(static_cast<char>(0xc0U | (code >> 6U))); text.push_back(static_cast<char>(0x80U | (code & 63U))); }
    else if (code < 0x10000) { text.push_back(static_cast<char>(0xe0U | (code >> 12U))); text.push_back(static_cast<char>(0x80U | ((code >> 6U) & 63U))); text.push_back(static_cast<char>(0x80U | (code & 63U))); }
    else { text.push_back(static_cast<char>(0xf0U | (code >> 18U))); text.push_back(static_cast<char>(0x80U | ((code >> 12U) & 63U))); text.push_back(static_cast<char>(0x80U | ((code >> 6U) & 63U))); text.push_back(static_cast<char>(0x80U | (code & 63U))); }
}
class GeoJson final {
    Reader& source; std::vector<std::uint8_t> data; std::size_t at = 0; std::uint64_t index = 0; std::string root_type;
    [[noreturn]] void bad() { throw Failure{ParseStatus::Malformed, "Invalid GeoJSON syntax or encoding", at}; }
    void whitespace() { while (at < data.size() && (data[at] == ' ' || data[at] == '\t' || data[at] == '\n' || data[at] == '\r')) { ++at; } }
    bool take(char c) { whitespace(); if (at < data.size() && data[at] == c) { ++at; return true; } return false; }
    std::string string() {
        whitespace(); if (at >= data.size() || data[at++] != '"') { bad(); } const auto start = at;
        bool escape = false, terminated = false; while (at < data.size()) { const auto c = data[at++]; if (!escape && c == '"') { terminated = true; break; } if (!escape && c < 0x20) { bad(); } escape = !escape && c == '\\'; }
        if (!terminated) { bad(); }
        const auto end = at - 1; if (!source.context.budget().reserve_allocation((end - start) * 4) || !source.context.budget().consume_decoded((end - start) * 4)) { throw Failure{ParseStatus::Internal, "GeoJSON string decoding budget", start}; }
        std::string text;
        auto codepoint = [&](std::size_t& position) {
            if (end - position < 4) { bad(); } std::uint32_t code = 0;
            for (unsigned i = 0; i < 4; ++i) { const auto c = data[position++]; const auto digit = c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : c >= 'A' && c <= 'F' ? c - 'A' + 10 : 99; if (digit > 15) { bad(); } code = code * 16 + static_cast<unsigned>(digit); } return code;
        };
        for (auto position = start; position < end;) {
            const auto c = data[position++]; if (c != '\\') { text.push_back(static_cast<char>(c)); continue; }
            if (position == end) { bad(); } const auto escaped = data[position++];
            if (escaped == '"' || escaped == '\\' || escaped == '/') { text.push_back(static_cast<char>(escaped)); }
            else if (escaped == 'b') { text.push_back('\b'); } else if (escaped == 'f') { text.push_back('\f'); } else if (escaped == 'n') { text.push_back('\n'); } else if (escaped == 'r') { text.push_back('\r'); } else if (escaped == 't') { text.push_back('\t'); }
            else if (escaped == 'u') {
                auto code = codepoint(position);
                if (code >= 0xd800 && code <= 0xdbff) { if (end - position < 6 || data[position++] != '\\' || data[position++] != 'u') { bad(); } const auto low = codepoint(position); if (low < 0xdc00 || low > 0xdfff) { bad(); } code = 0x10000 + ((code - 0xd800) << 10U) + low - 0xdc00; }
                else if (code >= 0xdc00 && code <= 0xdfff) { bad(); }
                append_utf8(text, code);
            } else { bad(); }
        }
        const auto decoded = decode_text(source, {reinterpret_cast<const std::uint8_t*>(text.data()), text.size()}, "UTF-8"); if (!decoded.valid) { bad(); } return text;
    }
    void value(std::uint32_t depth, const std::string& key, std::uint64_t parent) {
        source.entry(); if (!source.context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "GeoJSON depth limit", at}; }
        whitespace(); const auto start = at; if (at >= data.size()) { bad(); }
        const auto node = index++; auto field = source; field.group = "geo.json.node" + std::to_string(node); field.derived("parent", parent); field.decoded("key", ValueKind::Text, key, field.group, 0);
        if (take('{')) {
            field.decoded("type", ValueKind::Text, "object", field.group, 0); if (take('}')) { return; }
            do { const auto name = string(); if (!take(':')) { bad(); } value(depth + 1, name, node); } while (take(',')); if (!take('}')) { bad(); }
        } else if (take('[')) {
            field.decoded("type", ValueKind::Text, "array", field.group, 0); if (take(']')) { return; } std::uint64_t item = 0;
            do { value(depth + 1, std::to_string(item++), node); } while (take(',')); if (!take(']')) { bad(); }
        } else if (data[at] == '"') {
            const auto text = string(); if (depth == 1 && key == "type") { root_type = text; }
            field.put("value", ValueKind::Text, text, start, source.read(start, at - start), "UTF-8");
        } else {
            while (at < data.size() && data[at] != ',' && data[at] != ']' && data[at] != '}' && data[at] != ' ' && data[at] != '\n' && data[at] != '\r' && data[at] != '\t') { ++at; }
            const std::string token(data.begin() + static_cast<std::ptrdiff_t>(start), data.begin() + static_cast<std::ptrdiff_t>(at));
            if (token == "true" || token == "false" || token == "null") { field.put("literal", ValueKind::Text, token, start, source.read(start, at - start), "ASCII"); return; }
            std::size_t position = token.starts_with('-') ? 1 : 0; if (position >= token.size()) { bad(); }
            if (token[position] == '0') { ++position; } else { if (token[position] < '1' || token[position] > '9') { bad(); } while (position < token.size() && token[position] >= '0' && token[position] <= '9') { ++position; } }
            bool floating = false;
            if (position < token.size() && token[position] == '.') { floating = true; const auto first = ++position; while (position < token.size() && token[position] >= '0' && token[position] <= '9') { ++position; } if (position == first) { bad(); } }
            if (position < token.size() && (token[position] == 'e' || token[position] == 'E')) { floating = true; ++position; if (position < token.size() && (token[position] == '+' || token[position] == '-')) { ++position; } const auto first = position; while (position < token.size() && token[position] >= '0' && token[position] <= '9') { ++position; } if (position == first) { bad(); } }
            if (position != token.size()) { bad(); } auto kind = ValueKind::Bytes;
            if (floating) { double number = 0; const auto parsed = std::from_chars(token.data(), token.data() + token.size(), number); if (parsed.ec == std::errc{} && std::isfinite(number)) { kind = ValueKind::Float; } }
            else if (token.starts_with('-')) { std::int64_t number = 0; if (std::from_chars(token.data(), token.data() + token.size(), number).ec == std::errc{}) { kind = ValueKind::Signed; } }
            else { std::uint64_t number = 0; if (std::from_chars(token.data(), token.data() + token.size(), number).ec == std::errc{}) { kind = ValueKind::Unsigned; } }
            field.put("value", kind, token, start, source.read(start, at - start), "JSON number");
        }
    }
public:
    explicit GeoJson(Reader& r) : source(r), data(r.read(0, r.size)) {}
    ParseStatus run() {
        whitespace(); if (at >= data.size() || data[at] != '{') { return ParseStatus::Unsupported; } value(0, "", 0); whitespace(); if (at != data.size()) { bad(); }
        return root_type == "Feature" || root_type == "FeatureCollection" || root_type == "Point" || root_type == "LineString" || root_type == "Polygon" || root_type == "MultiPoint" || root_type == "MultiLineString" || root_type == "MultiPolygon" || root_type == "GeometryCollection" ? ParseStatus::Complete : ParseStatus::Unsupported;
    }
};

void shapefile(Reader& r) {
    r.require(0, 100); if (gnum(r, 28, 4) != 1000 || gnum(r, 24, 4, false) * 2 != r.size) { throw Failure{ParseStatus::Malformed, "Invalid shapefile header or length", 0}; }
    graw(r, "header_raw", 0, 100);
    r.number("shape_type", 32, 4, true); const char* bounds[] = {"x_min", "y_min", "x_max", "y_max", "z_min", "z_max", "m_min", "m_max"};
    for (unsigned i = 0; i < 8; ++i) { gdouble(r, bounds[i], 36 + i * 8); }
    std::uint64_t at = 100, index = 0;
    while (at < r.size) {
        r.entry(); r.require(at, 8); const auto length = gnum(r, at + 4, 4, false) * 2; auto record = r.window(at + 8, length, "geo.shp.record" + std::to_string(index++));
        auto record_header = r.window(at, 8, record.group); record_header.number("record_number", 0, 4); record_header.number("record_length_words", 4, 4);
        record.number("shape_type", 0, 4, true); const auto type = gnum(record, 0, 4); graw(record, "geometry_raw", 0, length);
        if (type == 1 || type == 11 || type == 21) { gdouble(record, "x", 4); gdouble(record, "y", 12); if (type == 11) { gdouble(record, "z", 20); } }
        else if (type == 3 || type == 5 || type == 13 || type == 15 || type == 23 || type == 25 || type == 31) {
            for (unsigned i = 0; i < 4; ++i) { gdouble(record, bounds[i], 4 + i * 8); }
            record.number("part_count", 36, 4, true); record.number("point_count", 40, 4, true);
            const auto parts = gnum(record, 36, 4), points = gnum(record, 40, 4); record.require(44, parts * (type == 31 ? 8 : 4) + points * 16);
        } else if (type == 8 || type == 18 || type == 28) {
            for (unsigned i = 0; i < 4; ++i) { gdouble(record, bounds[i], 4 + i * 8); } record.number("point_count", 36, 4, true); record.require(40, gnum(record, 36, 4) * 16);
        } else if (type != 0) { record.diagnostic("shape_type_unknown", "Unknown shape record retained without geometry interpretation", 0); }
        at += 8 + length;
    }
    r.derived("record_count", index); r.diagnostic("crs_unavailable", "Coordinates are file declarations; a projection sidecar is not assumed or fetched", 0);
}
void dbf(Reader& r) {
    r.require(0, 32); const auto rows = gnum(r, 4, 4), header = gnum(r, 8, 2), stride = gnum(r, 10, 2);
    if (header < 33 || !stride || (header - 33) % 32 || rows > (r.size - std::min(r.size, header)) / stride) { throw Failure{ParseStatus::Malformed, "Invalid DBF layout", 0}; }
    r.require(header - 1, 1); if (gnum(r, header - 1, 1) != 0x0d) { throw Failure{ParseStatus::Malformed, "DBF field terminator is missing", header - 1}; }
    r.number("version", 0, 1); graw(r, "updated_date_raw", 1, 3); r.number("record_count", 4, 4, true); r.number("code_page_mark", 29, 1);
    std::vector<std::uint64_t> widths; std::uint64_t total = 1;
    for (std::uint64_t at = 32; at + 32 < header; at += 32) {
        r.entry(); auto field = r.window(at, 32, "geo.dbf.field" + std::to_string(widths.size())); graw(field, "name_raw", 0, 11); graw(field, "type", 11, 1); field.number("length", 16, 1); field.number("decimals", 17, 1);
        const auto width = gnum(field, 16, 1); total += width; if (!r.context.budget().reserve_allocation(16)) { throw Failure{ParseStatus::Internal, "DBF field allocation limit", at}; } widths.push_back(width);
    }
    if (total != stride) { throw Failure{ParseStatus::Malformed, "DBF field widths differ from its record size", 0}; }
    for (std::uint64_t row = 0; row < rows; ++row) {
        r.entry(); auto record = r.window(header + row * stride, stride, "geo.dbf.record" + std::to_string(row)); record.number("deletion_marker", 0, 1); std::uint64_t at = 1;
        for (std::size_t column = 0; column < widths.size(); ++column) { record.entry(); graw(record, "field" + std::to_string(column), at, widths[column]); at += widths[column]; }
    }
}
const char* geokey_name(std::uint64_t key) {
    switch (key) { case 1024: return "model_type"; case 1025: return "raster_type"; case 1026: return "citation"; case 2048: return "geographic_crs_code"; case 2049: return "geographic_citation"; case 2050: return "datum_code"; case 2054: return "angular_units"; case 3072: return "projected_crs_code"; case 3073: return "projected_citation"; case 3075: return "coordinate_transform_code"; case 3076: return "linear_units"; case 4096: return "vertical_crs_code"; case 4099: return "vertical_units"; default: return nullptr; }
}
ParseStatus geotiff(Reader& r) {
    const auto header = r.read(0, 8); const bool little = starts(header, "II"); const auto magic = integer(header, 2, 2, little); const bool big = magic == 43;
    if (magic != 42 && magic != 43) { throw Failure{ParseStatus::Malformed, "Invalid GeoTIFF magic", 0}; }
    auto offset = integer(header, 4, 4, little); if (big) { r.require(0, 16); if (gnum(r, 4, 2, little) != 8 || gnum(r, 6, 2, little)) { throw Failure{ParseStatus::Malformed, "Invalid BigTIFF header", 0}; } offset = gnum(r, 8, 8, little); }
    const auto count_width = big ? 8U : 2U, stride = big ? 20U : 12U, inline_width = big ? 8U : 4U; std::set<std::uint64_t> visited; bool found = false;
    while (offset) {
        r.entry(); if (!r.context.budget().reserve_allocation(64)) { throw Failure{ParseStatus::Internal, "GeoTIFF directory allocation limit", offset}; }
        if (!visited.insert(offset).second) { throw Failure{ParseStatus::Malformed, "GeoTIFF directory cycle", offset}; }
        const auto count = gnum(r, offset, count_width, little); r.require(offset, count_width + inline_width);
        if (count > (r.size - offset - count_width - inline_width) / stride) { throw Failure{ParseStatus::Malformed, "GeoTIFF directory size overflow", offset}; }
        struct GeoTag { std::uint64_t at, count; unsigned width; };
        std::map<std::uint64_t, GeoTag> tags;
        for (std::uint64_t i = 0; i < count; ++i) {
            r.entry(); const auto at = offset + count_width + i * stride; const auto bytes = r.read(at, stride); const auto tag = integer(bytes, 0, 2, little), type = integer(bytes, 2, 2, little), quantity = integer(bytes, 4, big ? 8 : 4, little);
            if (tag != 33550 && tag != 33922 && tag != 34264 && tag != 34735 && tag != 34736 && tag != 34737) { continue; }
            const auto width = type == 3 ? 2U : type == 12 ? 8U : type == 2 ? 1U : 0U;
            if (!width || quantity > r.size / width) { throw Failure{ParseStatus::Malformed, "Invalid GeoTIFF tag type or size", at}; }
            const auto length = quantity * width, position = length <= inline_width ? at + (big ? 12 : 8) : integer(bytes, big ? 12 : 8, inline_width, little); r.require(position, length);
            if (!r.context.budget().reserve_allocation(80)) { throw Failure{ParseStatus::Internal, "GeoTIFF tag allocation limit", at}; }
            auto field = r; field.group = "geo.tiff.tag" + std::to_string(tag); graw(field, "raw", position, length);
            if (!tags.emplace(tag, GeoTag{position, quantity, width}).second) { throw Failure{ParseStatus::Malformed, "Duplicate GeoTIFF reference tag is ambiguous", at}; } found = true;
            if ((tag == 33550 && (width != 8 || quantity != 3)) || (tag == 33922 && (width != 8 || quantity % 6)) || (tag == 34264 && (width != 8 || quantity != 16)) || (tag == 34735 && width != 2) || (tag == 34736 && width != 8) || (tag == 34737 && width != 1)) { throw Failure{ParseStatus::Malformed, "GeoTIFF tag type or count does not match its definition", at}; }
            if (width == 8) { for (std::uint64_t j = 0; j < quantity; ++j) { field.entry(); gdouble(field, "value" + std::to_string(j), position + j * 8, little); } }
        }
        if (const auto keys = tags.find(34735); keys != tags.end()) {
            const auto& table = keys->second; if (table.width != 2 || table.count < 4) { throw Failure{ParseStatus::Malformed, "Invalid GeoKey directory", table.at}; }
            const auto key_count = gnum(r, table.at + 6, 2, little); if (key_count > (table.count - 4) / 4) { throw Failure{ParseStatus::Malformed, "GeoKey count exceeds its directory", table.at}; }
            for (std::uint64_t i = 0; i < key_count; ++i) {
                r.entry(); const auto at = table.at + 8 + i * 8, key = gnum(r, at, 2, little), location = gnum(r, at + 2, 2, little), quantity = gnum(r, at + 4, 2, little), value = gnum(r, at + 6, 2, little);
                const auto* name = geokey_name(key); auto field = r; field.group = "geo.tiff.key" + std::to_string(i); const auto key_name = name ? std::string(name) : hex_key(key);
                if (!location) { if (quantity != 1) { throw Failure{ParseStatus::Malformed, "Inline GeoKey has invalid count", at}; } field.put(key_name, ValueKind::Unsigned, std::to_string(value), at + 6, r.read(at + 6, 2), "", key, true); }
                else if (const auto target = tags.find(location); target != tags.end()) {
                    const auto& tag = target->second; if (value > tag.count || quantity > tag.count - value) { throw Failure{ParseStatus::Malformed, "GeoKey reference exceeds its tag", at}; }
                    const auto position = tag.at + value * tag.width; auto bytes = r.read(position, quantity * tag.width);
                    if (tag.width == 1) { const auto decoded = decode_text(r, bytes, "ASCII"); field.put(key_name, decoded.valid ? ValueKind::Text : ValueKind::Bytes, decoded.value, position, std::move(bytes), "ASCII", key, true); }
                    else { field.put(key_name, ValueKind::Bytes, "", position, std::move(bytes), "binary", key, true); }
                } else { graw(field, "reference_raw", at, 8); field.diagnostic("unknown_geokey_location", "Unknown GeoKey reference retained without a CRS interpretation", at); }
            }
        }
        offset = gnum(r, offset + count_width + count * stride, inline_width, little);
    }
    return found ? ParseStatus::Complete : ParseStatus::Unsupported;
}
ParseStatus geo(Reader& r) {
    const auto header = r.read(0, std::min<std::uint64_t>(32, r.size));
    if (starts(header, "II") || starts(header, "MM")) { return geotiff(r); }
    if (header.size() >= 4 && integer(header, 0, 4) == 9994) { r.group = "geo.shp"; shapefile(r); return ParseStatus::Complete; }
    if (header.size() == 32 && (header[0] == 3 || header[0] == 0x83 || header[0] == 0x8b || header[0] == 0xf5)) { r.group = "geo.dbf"; dbf(r); return ParseStatus::Complete; }
    const auto first = std::find_if(header.begin(), header.end(), [](unsigned char c) { return c != ' ' && c != '\t' && c != '\r' && c != '\n'; });
    if (first != header.end() && *first == '{') { return GeoJson(r).run(); }
    if (first != header.end() && *first == '<') { const auto bytes = r.read(0, r.size); inspect_xml_fields(r, bytes, "geo.xml", "gpx", "kml"); return ParseStatus::Complete; }
    return ParseStatus::Unsupported;
}
}
}
namespace metaxtract::native {
namespace {
class GeoParser final : public Parser {
    const ParserCapability& capability() const noexcept override { static const ParserCapability value{"formats.geo", "Geospatial file and coordinate declarations", "1", Extract, {"geojson", "gpx", "kml", "shp", "dbf", "geotiff"}}; return value; }
    ParseStatus run(ParserContext& context) const override {
        formats::Reader r(context, 0, context.source_size(), "geo");
        try { return formats::geo(r); } catch (const formats::Failure& error) { r.diagnostic("failure", error.message, 0); return error.status; }
    }
};
}
std::unique_ptr<Parser> geo_metadata_parser() { return std::make_unique<GeoParser>(); }
}
