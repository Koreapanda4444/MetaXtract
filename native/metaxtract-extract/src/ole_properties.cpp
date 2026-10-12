#include "ole.hpp"

#include <bit>
#include <cmath>
#include <map>

namespace metaxtract::native::formats {
namespace {
std::string property_label(const std::span<const std::uint8_t> guid, const std::uint32_t id) {
    constexpr std::string_view summary("\xe0\x85\x9f\xf2\xf9\x4f\x68\x10\xab\x91\x08\x00\x2b\x27\xb3\xd9", 16);
    constexpr std::string_view document("\x02\xd5\xcd\xd5\x9c\x2e\x1b\x10\x93\x97\x08\x00\x2b\x2c\xf9\xae", 16);
    if (id == 1) { return "code_page"; }
    if (starts(guid, summary)) {
        static constexpr const char* labels[] = {"dictionary","code_page","title","subject","author","keywords","comments",
            "template","last_saved_by","revision_number","editing_time_filetime","last_printed_filetime",
            "created_filetime","last_saved_filetime","page_count","word_count","character_count","thumbnail","application","security"};
        if (id < std::size(labels)) { return labels[id]; }
    } else if (starts(guid, document)) {
        static constexpr const char* labels[] = {"dictionary","code_page","category","presentation_target","byte_count","line_count",
            "paragraph_count","slide_count","note_count","hidden_slide_count","multimedia_clip_count","scale_crop",
            "heading_pairs","document_parts","manager","company","links_dirty","character_count_with_spaces"};
        if (id < std::size(labels)) { return labels[id]; }
    }
    return "property_" + std::to_string(id);
}
void emit_text(Reader& reader, const std::string& key, const std::span<const std::uint8_t> bytes,
               const std::uint32_t page, const std::string& packet) {
    auto text = decode_text(reader, bytes, std::to_string(page));
    if (!text.supported) { throw Failure{ParseStatus::Unsupported, "Property code page is unsupported; original bytes retained", 0}; }
    if (!text.valid) { throw Failure{ParseStatus::Malformed, "Invalid property string encoding", 0}; }
    if (!text.value.empty() && text.value.back() == '\0') { text.value.pop_back(); }
    reader.decoded(key, ValueKind::Text, std::move(text.value), packet);
}

std::uint64_t scalar(Reader& reader, const std::uint32_t type, const std::uint64_t at,
                     const std::string& key, const std::uint32_t page, const std::string& packet) {
    unsigned width = 0;
    bool signed_value = false, floating = false;
    switch (type) {
    case 0: case 1: return 0;
    case 2: width = 2; signed_value = true; break;
    case 3: case 22: width = 4; signed_value = true; break;
    case 4: width = 4; floating = true; break;
    case 5: case 7: width = 8; floating = true; break;
    case 6: case 20: width = 8; signed_value = true; break;
    case 10: case 19: case 23: width = 4; break;
    case 11: width = 2; break;
    case 16: width = 1; signed_value = true; break;
    case 17: width = 1; break;
    case 18: width = 2; break;
    case 21: case 64: width = 8; break;
    case 8: case 30: case 31: {
        const auto length = integer(reader.read(at, 4), 0, 4, true);
        const auto amount = length * (type == 31 ? 2ULL : 1ULL);
        const auto bytes = reader.read(at + 4, amount);
        if (amount != 0 && (type == 31 || page == 1200) && (amount < 2 || integer(bytes, bytes.size() - 2, 2, true) != 0)) {
            throw Failure{ParseStatus::Malformed, "Unicode property string has no terminator", 0};
        }
        if (amount != 0 && type != 31 && page != 1200 && bytes.back() != 0) {
            throw Failure{ParseStatus::Malformed, "Property string has no terminator", 0};
        }
        emit_text(reader, key, bytes, type == 31 ? 1200 : page, packet);
        return 4 + amount;
    }
    case 65: case 70: case 71: {
        const auto length = integer(reader.read(at, 4), 0, 4, true);
        reader.put(key, ValueKind::Bytes, "", at + 4, reader.read(at + 4, length), "binary");
        return 4 + length;
    }
    case 72:
        reader.put(key, ValueKind::Bytes, "", at, reader.read(at, 16), "GUID"); return 16;
    default: throw Failure{ParseStatus::Unsupported, "Property value type is unsupported; raw value retained", 0};
    }
    auto bytes = reader.read(at, width);
    const auto raw = integer(bytes, 0, width, true);
    if (type == 11 && raw != 0 && raw != 0xffff) { throw Failure{ParseStatus::Malformed, "Invalid VARIANT_BOOL property", 0}; }
    if (floating) {
        const auto value = width == 4 ? static_cast<double>(std::bit_cast<float>(static_cast<std::uint32_t>(raw))) : std::bit_cast<double>(raw);
        if (!std::isfinite(value)) { throw Failure{ParseStatus::Malformed, "Non-finite property number", 0}; }
        std::ostringstream formatted; formatted << std::setprecision(17) << value;
        reader.put(key, ValueKind::Float, formatted.str(), at, std::move(bytes), type == 7 ? "OLE_DATE_days" : "IEEE754");
    } else if (signed_value) {
        std::int64_t value = std::bit_cast<std::int64_t>(raw);
        if (width < 8 && (raw & (std::uint64_t{1} << (width * 8 - 1))) != 0) {
            value = std::bit_cast<std::int64_t>(raw | (~std::uint64_t{0} << (width * 8)));
        }
        reader.put(key, ValueKind::Signed, std::to_string(value), at, std::move(bytes), type == 6 ? "currency_1e4" : "");
    } else {
        reader.put(key, ValueKind::Unsigned, std::to_string(raw), at, std::move(bytes), type == 64 ? "FILETIME_100ns" : "");
    }
    return width;
}

void typed(Reader& reader, const std::uint64_t at, const std::string& key,
           const std::uint32_t page, const std::string& packet, const std::uint32_t depth) {
    reader.entry();
    if (!reader.context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "Property value depth budget exceeded", 0}; }
    const auto type = integer(reader.read(at, 4), 0, 4, true);
    reader.derived("value_type", type);
    if ((type & 0xffff0000U) != 0) { throw Failure{ParseStatus::Malformed, "Nonzero typed property padding", 0}; }
    if ((type & 0x1000U) != 0) {
        const auto count = integer(reader.read(at + 4, 4), 0, 4, true);
        reader.derived("vector_count", count);
        const auto base_type = static_cast<std::uint32_t>(type & 0xfffU);
        std::uint64_t cursor = at + 8;
        for (std::uint64_t index = 0; index < count; ++index) {
            reader.entry(); reader.checkpoint();
            if (base_type == 12) {
                // Variant vectors have heterogeneous, individually padded values.
                const auto nested_type = integer(reader.read(cursor, 4), 0, 4, true);
                if (nested_type > 0xfff || (nested_type & 0xffff0000U) != 0) {
                    throw Failure{ParseStatus::Unsupported, "Nested property vector retained without recursive interpretation", 0};
                }
                const auto used = scalar(reader, static_cast<std::uint32_t>(nested_type), cursor + 4, key, page, packet);
                cursor += 4 + used; cursor = (cursor + 3) & ~std::uint64_t{3};
            } else {
                const auto used = scalar(reader, base_type, cursor, key, page, packet);
                if (used == 0 && count != 0) { throw Failure{ParseStatus::Malformed, "Empty/null property vector element", 0}; }
                cursor += used;
                if (base_type == 8 || base_type == 30 || base_type == 31 || base_type == 65 || base_type == 70) {
                    cursor = (cursor + 3) & ~std::uint64_t{3};
                }
            }
            reader.require(cursor, 0);
        }
    } else {
        static_cast<void>(scalar(reader, static_cast<std::uint32_t>(type), at + 4, key, page, packet));
    }
}
}

void inspect_properties(Reader& reader, const std::string& packet) {
    const auto header = reader.read(0, 28);
    if (integer(header, 0, 2, true) != 0xfffe) { throw Failure{ParseStatus::Malformed, "Invalid property-set byte order", 0}; }
    reader.number("property_set_version", 2, 2, true); reader.number("property_set_system", 4, 4, true);
    reader.put("property_set_class_id", ValueKind::Bytes, "", 8, reader.read(8, 16), "GUID");
    const auto count = integer(header, 24, 4, true);
    reader.require(28, 20 * count);
    ParseStatus status = ParseStatus::Complete;
    for (std::uint64_t section = 0; section < count; ++section) {
        reader.entry(); reader.checkpoint();
        const auto descriptor = reader.read(28 + 20 * section, 20);
        const auto offset = integer(descriptor, 16, 4, true);
        if (offset < 28 + 20 * count || offset % 4 != 0) { throw Failure{ParseStatus::Malformed, "Property section overlaps its descriptor table", 0}; }
        const auto size = integer(reader.read(offset, 4), 0, 4, true);
        auto set = reader.window(offset, size, reader.group + ".set" + std::to_string(section));
        const auto properties = integer(set.read(4, 4), 0, 4, true);
        set.require(8, properties * 8);
        const auto guid = std::span(descriptor).first(16);
        set.put("format_id", ValueKind::Bytes, "", 0, {guid.begin(), guid.end()}, "GUID");
        struct Property { std::uint32_t id; std::uint64_t at; std::uint64_t index; };
        if (!reader.context.budget().reserve_allocation(properties * 64)) { throw Failure{ParseStatus::Internal, "Property index allocation budget exceeded", 0}; }
        std::vector<Property> items;
        std::uint32_t page = 0;
        for (std::uint64_t index = 0; index < properties; ++index) {
            set.entry();
            const auto entry = set.read(8 + index * 8, 8);
            const auto id = static_cast<std::uint32_t>(integer(entry, 0, 4, true));
            const auto at = integer(entry, 4, 4, true);
            if (at < 8 + properties * 8 || at >= size || at % 4 != 0) { throw Failure{ParseStatus::Malformed, "Property value offset escapes its section", 0}; }
            items.push_back({id, at, index});
            if (id == 1) {
                const auto value = set.read(at, 8);
                if (integer(value, 0, 4, true) != 2) { throw Failure{ParseStatus::Malformed, "Code-page property is not VT_I2", 0}; }
                page = static_cast<std::uint32_t>(integer(value, 4, 2, true));
                set.derived("declared_code_page", page);
            }
        }
        std::sort(items.begin(), items.end(), [](const auto& a, const auto& b) { return a.at < b.at; });
        for (std::size_t position = 0; position < items.size(); ++position) {
            const auto& item = items[position];
            const auto end = position + 1 < items.size() ? items[position + 1].at : size;
            auto value = set.window(item.at, end - item.at, set.group + ".property" + std::to_string(item.index));
            value.derived("property_id", item.id);
            value.put("raw_property", ValueKind::Bytes, "", 0, value.read(0, value.size), "binary", item.id, true);
            const auto label = property_label(guid, item.id);
            value.decoded("property_name", ValueKind::Text, label, packet);
            try {
                if (item.id == 0) {
                    const auto names = integer(value.read(0, 4), 0, 4, true);
                    std::uint64_t at = 4;
                    for (std::uint64_t index = 0; index < names; ++index) {
                        value.entry();
                        const auto record = value.read(at, 8);
                        const auto id = integer(record, 0, 4, true), length = integer(record, 4, 4, true);
                        const auto amount = length * (page == 1200 ? 2ULL : 1ULL);
                        value.derived("dictionary_property_id", id);
                        const auto bytes = value.read(at + 8, amount);
                        emit_text(value, "dictionary_name", bytes, page, packet);
                        at += 8 + amount;
                        if (page == 1200) { at = (at + 3) & ~std::uint64_t{3}; }
                    }
                } else { typed(value, 0, label, page, packet, 1); }
            } catch (const Failure& error) {
                value.diagnostic("property_failure", error.message, 0);
                if (reader.context.budget().status() != BudgetStatus::Ok) { throw; }
                if (error.status == ParseStatus::Malformed || status == ParseStatus::Complete) { status = error.status; }
            }
        }
    }
    if (status != ParseStatus::Complete) { throw Failure{status, "Some property values require unsupported decoding or are malformed; raw values retained", 0}; }
}
}
