#include "format.hpp"
#include "xml.hpp"
#include <qpdf/QPDFCryptoProvider.hh>
#include <map>
#include <set>

namespace metaxtract::native::formats {
namespace {
std::string guid(std::string value) {
    if (value.size() != 32) { throw Failure{ParseStatus::Malformed, "Extended XMP GUID length is invalid", 0}; }
    for (auto& c : value) {
        if (c >= 'a' && c <= 'f') { c = static_cast<char>(c - 'a' + 'A'); }
        if (!((c >= '0' && c <= '9') || (c >= 'A' && c <= 'F'))) {
            throw Failure{ParseStatus::Malformed, "Extended XMP GUID is not hexadecimal", 0};
        }
    }
    return value;
}
void references(Reader& r, std::span<const std::uint8_t> bytes, std::set<std::string>& ids) {
    XmlBudgetScope scope(r.context.budget());
    pugi::xml_document document;
    if (!document.load_buffer(bytes.data(), bytes.size(), pugi::parse_default, pugi::encoding_auto)) {
        throw Failure{ParseStatus::Malformed, "Invalid standard XMP packet", r.base};
    }
    constexpr auto key = "{http://ns.adobe.com/xmp/note/}HasExtendedXMP";
    const auto retain = [&](const std::string& value) {
        if (!r.context.budget().reserve_allocation(256)) { throw Failure{ParseStatus::Internal, "XMP reference allocation limit", r.base}; }
        ids.insert(guid(value));
    };
    auto node = document.first_child(); std::uint32_t depth = 1;
    while (node) {
        r.entry();
        if (!r.context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "XMP reference depth limit", r.base}; }
        if (node.type() == pugi::node_element) {
            if (expanded_xml_name(node, node.name()) == key) { retain(node.child_value()); }
            for (auto attr : node.attributes()) {
                r.entry();
                if (expanded_xml_name(node, attr.name(), true) == key) { retain(attr.value()); }
            }
        }
        if (node.first_child()) { node = node.first_child(); ++depth; }
        else { while (node && !node.next_sibling()) { node = node.parent(); if (depth) { --depth; } } if (node) { node = node.next_sibling(); } }
    }
}
struct Fragment { std::uint64_t source, offset, length, total; };
}

void inspect_extended_xmp(Reader& r, const std::vector<std::pair<std::uint64_t, std::uint64_t>>& packets) {
    std::set<std::string> links;
    std::map<std::string, std::vector<Fragment>> fragments;
    for (const auto& [at, length] : packets) {
        r.checkpoint();
        const auto prefix = r.read(at, std::min<std::uint64_t>(length, 75));
        if (starts(prefix, std::string_view("http://ns.adobe.com/xap/1.0/\0", 29))) {
            auto bytes = r.read(at + 29, length - 29); references(r, bytes, links);
        } else if (starts(prefix, std::string_view("http://ns.adobe.com/xmp/extension/\0", 35))) {
            if (length <= 75) { throw Failure{ParseStatus::Malformed, "Truncated Extended XMP fragment", r.base + at}; }
            const auto id = guid(std::string(prefix.begin() + 35, prefix.begin() + 67));
            const auto total = integer(prefix, 67, 4), offset = integer(prefix, 71, 4);
            if (!total || offset > total || length - 75 > total - offset) {
                throw Failure{ParseStatus::Malformed, "Extended XMP fragment escapes its packet", r.base + at};
            }
            if (!r.context.budget().check_member_size(total)) { throw Failure{ParseStatus::Internal, "Extended XMP packet size limit", r.base + at}; }
            fragments[id].push_back({at + 75, offset, length - 75, total});
        }
    }
    for (auto& [id, parts] : fragments) {
        if (!links.contains(id)) { r.diagnostic("orphan_extended_xmp", "Unlinked Extended XMP fragments retained without merging", parts.front().source); continue; }
        std::sort(parts.begin(), parts.end(), [](const auto& a, const auto& b) { return a.offset < b.offset; });
        std::uint64_t next = 0;
        const auto total = parts.front().total;
        for (const auto& part : parts) {
            r.entry();
            if (part.total != total || part.offset != next) { throw Failure{ParseStatus::Malformed, "Overlapping, missing or contradictory Extended XMP fragments", r.base + part.source}; }
            next += part.length;
        }
        if (next != total) { throw Failure{ParseStatus::Malformed, "Incomplete Extended XMP packet", r.base}; }
        if (!r.context.budget().reserve_allocation(total * 3) || !r.context.budget().consume_decoded(total)) {
            throw Failure{ParseStatus::Internal, "Extended XMP assembly budget", r.base};
        }
        std::vector<std::uint8_t> bytes; bytes.reserve(static_cast<std::size_t>(total));
        for (const auto& part : parts) { const auto data = r.read(part.source, part.length); bytes.insert(bytes.end(), data.begin(), data.end()); }
        if (!r.context.budget().reserve_allocation(4096)) { throw Failure{ParseStatus::Internal, "XMP digest allocation limit", r.base}; }
        auto hash = QPDFCryptoProvider::getImpl(); hash->MD5_init();
        for (std::size_t at = 0; at < bytes.size(); at += 65536) { r.checkpoint(); hash->MD5_update(bytes.data() + at, std::min<std::size_t>(65536, bytes.size() - at)); }
        hash->MD5_finalize();
        QPDFCryptoImpl::MD5_Digest digest{}; hash->MD5_digest(digest);
        constexpr char hex[] = "0123456789ABCDEF"; std::string actual;
        for (const auto byte : digest) { actual += hex[byte >> 4U]; actual += hex[byte & 15U]; }
        if (actual != id) { throw Failure{ParseStatus::Malformed, "Extended XMP MD5 identifier mismatch", r.base}; }
        auto extended = r.window(0, r.size, r.group + ".extended_xmp");
        const auto packet = "jpeg-extended-xmp:" + id;
        extended.decoded("assembled_guid", ValueKind::Text, id, packet);
        extended.decoded("assembled_packet", ValueKind::DecodedBytes, std::string(bytes.begin(), bytes.end()), packet);
        inspect_xmp_data(extended, bytes, packet);
    }
    for (const auto& id : links) {
        if (!fragments.contains(id)) { throw Failure{ParseStatus::Malformed, "Referenced Extended XMP packet is missing", r.base}; }
    }
}
}
