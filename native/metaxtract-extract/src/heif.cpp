#include "boxes.hpp"

#include <map>
#include <memory>
#include <set>

namespace metaxtract::native {
namespace {

using namespace formats;

struct Item final {
    std::string type, name, mime, encoding;
    std::uint16_t protection = 0;
    std::uint16_t method = 0, reference = 0;
    std::uint64_t base = 0;
    std::vector<std::pair<std::uint64_t, std::uint64_t>> extents;
    std::vector<std::uint32_t> properties;
};

class Heif final {
    Reader& file_;
    std::map<std::uint32_t, Item> items_;
    std::vector<Box> properties_;
    std::uint64_t idat_offset_ = 0, idat_size_ = 0;
    std::uint32_t primary_ = 0;
    std::uint64_t number(Reader& reader, std::uint64_t& at, unsigned width) {
        const auto result = integer(reader.read(at, width), 0, width); at += width; return result;
    }
    std::string string(Reader& reader, std::uint64_t& at) {
        std::string result;
        while (at < reader.size) {
            reader.checkpoint();
            const auto byte = reader.read(at++, 1)[0];
            if (byte == 0) { return result; }
            if (!reader.context.budget().reserve_allocation(2)) { throw Failure{ParseStatus::Internal, "HEIF string allocation", reader.base + at}; }
            result += static_cast<char>(byte);
        }
        throw Failure{ParseStatus::Malformed, "HEIF string terminator missing", reader.base + at};
    }
    Item& item(const std::uint32_t id) {
        if (!items_.contains(id) && !file_.context.budget().reserve_allocation(1024)) { throw Failure{ParseStatus::Internal, "HEIF item allocation", file_.base}; }
        return items_[id];
    }
    void infe(Reader& body) {
        const auto header = body.read(0, 4); const auto version = header[0];
        std::uint64_t at = 4;
        if (version > 3) { throw Failure{ParseStatus::Unsupported, "Unsupported HEIF item-info version", body.base}; }
        const auto id = static_cast<std::uint32_t>(number(body, at, version == 3 ? 4 : 2));
        auto& current = item(id); current.protection = static_cast<std::uint16_t>(number(body, at, 2));
        if (version >= 2) { const auto bytes = body.read(at, 4); current.type.assign(bytes.begin(), bytes.end()); at += 4; }
        current.name = string(body, at);
        if (version < 2 || current.type == "mime") { current.mime = string(body, at); if (at < body.size) { current.encoding = string(body, at); } }
        else if (current.type == "uri ") { current.mime = string(body, at); }
        body.derived("item_id", id);
        body.decoded("item_type", ValueKind::Text, current.type, "heif-item-" + std::to_string(id));
        body.decoded("item_name", ValueKind::Text, current.name, "heif-item-" + std::to_string(id));
        body.decoded("item_mime", ValueKind::Text, current.mime, "heif-item-" + std::to_string(id));
        body.derived("protection_index", current.protection);
    }
    void iloc(Reader& body) {
        const auto header = body.read(0, 6); const auto version = header[0];
        if (version > 2) { throw Failure{ParseStatus::Unsupported, "Unsupported HEIF item-location version", body.base}; }
        const unsigned offset_width = header[4] >> 4U, length_width = header[4] & 15U, base_width = header[5] >> 4U;
        const unsigned index_width = version == 0 ? 0 : header[5] & 15U;
        if (offset_width > 8 || length_width > 8 || base_width > 8 || index_width > 8) { throw Failure{ParseStatus::Malformed, "Invalid HEIF extent integer width", body.base + 4}; }
        std::uint64_t at = 6;
        const auto count = number(body, at, version < 2 ? 2 : 4);
        for (std::uint64_t index = 0; index < count; ++index) {
            body.entry();
            const auto id = static_cast<std::uint32_t>(number(body, at, version < 2 ? 2 : 4));
            auto& current = item(id);
            if (!current.extents.empty()) { throw Failure{ParseStatus::Malformed, "Duplicate HEIF item location", body.base + at}; }
            if (version != 0) { current.method = static_cast<std::uint16_t>(number(body, at, 2) & 15U); }
            current.reference = static_cast<std::uint16_t>(number(body, at, 2));
            current.base = number(body, at, base_width);
            const auto extents = number(body, at, 2);
            for (std::uint64_t extent = 0; extent < extents; ++extent) {
                body.entry();
                if (index_width != 0) { static_cast<void>(number(body, at, index_width)); }
                const auto offset = number(body, at, offset_width), length = number(body, at, length_width);
                if (!body.context.budget().reserve_allocation(128)) { throw Failure{ParseStatus::Internal, "HEIF extent allocation", body.base + at}; }
                current.extents.emplace_back(offset, length);
            }
        }
        if (at != body.size) { body.diagnostic("iloc_trailing", "Bytes remain after the item-location table", at); }
    }
    void ipma(Reader& body) {
        const auto header = body.read(0, 4); const auto version = header[0];
        if (version > 1) { throw Failure{ParseStatus::Unsupported, "Unsupported HEIF property-association version", body.base}; }
        const auto wide = (integer(header, 1, 3) & 1U) != 0;
        std::uint64_t at = 4; const auto count = number(body, at, 4);
        for (std::uint64_t index = 0; index < count; ++index) {
            body.entry();
            const auto id = static_cast<std::uint32_t>(number(body, at, version == 0 ? 2 : 4));
            auto& current = item(id); const auto associations = number(body, at, 1);
            for (std::uint64_t association = 0; association < associations; ++association) {
                body.entry(); const auto value = number(body, at, wide ? 2 : 1);
                if (!body.context.budget().reserve_allocation(16)) { throw Failure{ParseStatus::Internal, "HEIF property-association allocation", body.base + at}; }
                current.properties.push_back(static_cast<std::uint32_t>(value & (wide ? 0x7fffU : 0x7fU)));
            }
        }
    }
    void property(Reader& reader, const Box& box) {
        auto body = reader.window(box.offset + box.header, box.size - box.header, reader.group + "." + box.type);
        if (box.type == "ispe") { if (body.size != 12) { throw Failure{ParseStatus::Malformed, "Invalid HEIF spatial extent", body.base}; } body.number("width", 4, 4); body.number("height", 8, 4); }
        else if (box.type == "irot" || box.type == "imir") { if (body.size != 1) { throw Failure{ParseStatus::Malformed, "Invalid HEIF transform", body.base}; } body.number(box.type == "irot" ? "rotation_quarters" : "mirror_axis", 0, 1); }
        else if (box.type == "pixi") {
            const auto data = body.read(0, 5); body.require(5, data[4]); body.number("channels", 4, 1);
            for (std::uint64_t index = 0; index < data[4]; ++index) { body.number("bits_per_channel", 5 + index, 1); }
        } else if (box.type == "colr") {
            const auto type = body.read(0, 4);
            body.put("color_type", ValueKind::Text, std::string(type.begin(), type.end()), 0, type, "ASCII");
            if (starts(type, "prof") || starts(type, "rICC")) { auto profile = body.window(4, body.size - 4, body.group + ".icc"); inspect_icc(profile); }
            else if (starts(type, "nclx")) { body.require(4, 7); body.number("color_primaries", 4, 2); body.number("transfer_characteristics", 6, 2); body.number("matrix_coefficients", 8, 2); body.number("full_range_flag", 10, 1); }
            else { body.put("payload", ValueKind::Bytes, "", 4, body.read(4, body.size - 4)); }
        } else { body.put("payload", ValueKind::Bytes, "", 0, body.read(0, body.size)); }
    }
    void meta(Reader& reader, const std::uint32_t depth) {
        if (!reader.context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "HEIF box depth", reader.base}; }
        walk_boxes(reader, 0, reader.size, [&](const Box& box) {
            auto body = reader.window(box.offset + box.header, box.size - box.header, reader.group + "." + box.type);
            if (box.type == "infe") { infe(body); }
            else if (box.type == "iloc") { iloc(body); }
            else if (box.type == "pitm") {
                const auto version = body.read(0, 4)[0];
                if (version > 1) { throw Failure{ParseStatus::Unsupported, "Unsupported HEIF primary-item version", body.base}; }
                primary_ = static_cast<std::uint32_t>(integer(body.read(4, version == 0 ? 2 : 4), 0, version == 0 ? 2 : 4));
                body.derived("primary_item", primary_);
            } else if (box.type == "idat") { idat_offset_ = body.base; idat_size_ = body.size; }
            else if (box.type == "iinf") {
                const auto version = body.read(0, 4)[0];
                if (version > 1) { throw Failure{ParseStatus::Unsupported, "Unsupported HEIF item-info table version", body.base}; }
                const auto skip = version == 0 ? 6U : 8U;
                auto child = body.window(skip, body.size - std::min<std::uint64_t>(skip, body.size), body.group); meta(child, depth + 1);
            } else if (box.type == "iprp") { meta(body, depth + 1); }
            else if (box.type == "ipco") {
                walk_boxes(body, 0, body.size, [&](const Box& value) {
                    if (!body.context.budget().reserve_allocation(256)) { throw Failure{ParseStatus::Internal, "HEIF property table allocation", body.base}; }
                    properties_.push_back({body.base + value.offset, value.header, value.size, value.type});
                });
            } else if (box.type == "ipma") { ipma(body); }
            else if (box.type == "iref") {
                const auto version = body.read(0, 4)[0];
                if (version > 1) { throw Failure{ParseStatus::Unsupported, "Unsupported HEIF reference version", body.base}; }
                walk_boxes(body, 4, body.size, [&](const Box& value) {
                    auto relation = body.window(value.offset + value.header, value.size - value.header, body.group + "." + value.type);
                    std::uint64_t at = 0; const auto from = number(relation, at, version == 0 ? 2 : 4), count = number(relation, at, 2);
                    relation.derived("from_item", from);
                    for (std::uint64_t index = 0; index < count; ++index) { relation.entry(); relation.derived("to_item", number(relation, at, version == 0 ? 2 : 4)); }
                });
            } else if (box.type == "hdlr") {
                body.require(0, 24); const auto type = body.read(8, 4);
                body.put("handler_type", ValueKind::Text, std::string(type.begin(), type.end()), 8, type, "ASCII");
                body.put("handler_name", ValueKind::Bytes, "", 24, body.read(24, body.size - 24));
            } else { body.put("unknown_box", ValueKind::Bytes, "", 0, body.read(0, body.size)); }
        });
    }
public:
    explicit Heif(Reader& file) : file_(file) {}
    ParseStatus run() {
        bool brand = false, found = false;
        walk_boxes(file_, 0, file_.size, [&](const Box& box) {
            auto body = file_.window(box.offset + box.header, box.size - box.header, file_.group + "." + box.type);
            if (box.type == "ftyp") {
                const auto data = body.read(0, body.size); if (data.size() < 8 || data.size() % 4 != 0) { throw Failure{ParseStatus::Malformed, "Invalid HEIF brands", body.base}; }
                for (std::size_t at = 0; at < data.size(); at += 4) {
                    if (at == 4) { continue; }
                    const std::string value(data.begin() + static_cast<std::ptrdiff_t>(at), data.begin() + static_cast<std::ptrdiff_t>(at + 4));
                    if (value == "mif1" || value == "msf1" || value == "heic" || value == "heix" || value == "hevc" || value == "hevx" || value == "avif" || value == "avis") { brand = true; }
                    body.put(at == 0 ? "major_brand" : "compatible_brand", ValueKind::Text, value, at, {data.begin() + static_cast<std::ptrdiff_t>(at), data.begin() + static_cast<std::ptrdiff_t>(at + 4)}, "ASCII");
                }
            } else if (box.type == "meta") {
                if (body.read(0, 4)[0] != 0) { throw Failure{ParseStatus::Unsupported, "Unsupported HEIF meta version", body.base}; }
                auto child = body.window(4, body.size - 4, body.group); meta(child, 1); found = true;
            }
        });
        if (!brand) { return ParseStatus::Unsupported; }
        if (!found || items_.empty()) { throw Failure{ParseStatus::Malformed, "HEIF item metadata missing", file_.base}; }
        for (const auto& [id, current] : items_) {
            file_.entry(); auto owner = file_.window(0, file_.size, file_.group + ".item" + std::to_string(id));
            owner.derived("is_primary", id == primary_ ? 1 : 0);
            for (const auto index : current.properties) {
                if (index == 0) { continue; }
                if (index > properties_.size()) { throw Failure{ParseStatus::Malformed, "HEIF property index outside table", file_.base}; }
                property(owner, properties_[index - 1]);
            }
            if (current.type != "Exif" && current.mime != "application/rdf+xml" && current.mime != "application/xml") { continue; }
            if (current.protection != 0 || current.reference != 0 || current.method > 1 || !current.encoding.empty()) { owner.diagnostic("unavailable_item", "Protected, external, encoded or referenced metadata item retained without decoding", 0); continue; }
            std::vector<std::uint8_t> joined;
            std::uint64_t first_offset = 0, first_length = 0;
            for (const auto& [offset, length] : current.extents) {
                owner.entry();
                if (length == 0) { throw Failure{ParseStatus::Unsupported, "Unspecified metadata extent length", 0}; }
                if (offset > UINT64_MAX - current.base) { throw Failure{ParseStatus::Malformed, "HEIF extent offset overflow", 0}; }
                const auto relative = offset + current.base;
                if (current.method == 1 && (relative > idat_size_ || length > idat_size_ - relative)) { throw Failure{ParseStatus::Malformed, "HEIF extent escapes idat", idat_offset_}; }
                const auto absolute = relative + (current.method == 1 ? idat_offset_ : 0);
                auto bytes = file_.read(absolute, length);
                if (current.extents.size() == 1) { first_offset = absolute; first_length = length; }
                owner.put("metadata_extent", ValueKind::Bytes, "", absolute, bytes);
                if (current.extents.size() != 1) {
                    if (!owner.context.budget().reserve_allocation(length * 2)) { throw Failure{ParseStatus::Internal, "HEIF metadata assembly budget", absolute}; }
                    joined.insert(joined.end(), bytes.begin(), bytes.end());
                }
            }
            if (joined.empty() && first_length == 0) { owner.diagnostic("missing_extent", "Metadata item has no available extent", 0); continue; }
            auto packet = first_length != 0 ? file_.window(first_offset, first_length, owner.group + ".metadata")
                : Reader(owner.context, joined, owner.group + ".metadata", "heif-item-" + std::to_string(id));
            if (current.type == "Exif") {
                const auto skip = integer(packet.read(0, 4), 0, 4);
                packet.require(4, skip);
                auto exif = packet.window(4 + skip, packet.size - 4 - skip, packet.group + ".exif"); inspect_tiff(exif, true);
            } else { inspect_xmp(packet); }
        }
        file_.derived("item_count", items_.size()); return ParseStatus::Complete;
    }
};

class HeifParser final : public Parser {
public:
    const ParserCapability& capability() const noexcept override {
        static const ParserCapability value{"images.heif", "MetaXtract HEIF item metadata parser", "builtin", Probe | Extract,
            {"heif", "heic", "avif"}}; return value;
    }
    ParseStatus run(ParserContext& context) const override {
        Reader reader(context, 0, context.source_size(), "image.heif");
        try { Heif parser(reader); return parser.run(); }
        catch (const Failure& failure) { if (context.budget().status() == BudgetStatus::Ok) { reader.diagnostic("heif_error", failure.message, failure.offset); } return failure.status; }
    }
};

}

std::unique_ptr<Parser> heif_metadata_parser() { return std::make_unique<HeifParser>(); }

}
