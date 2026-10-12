#include "xml.hpp"

#include <cstdlib>
#include <mutex>

namespace metaxtract::native::formats {
namespace {
thread_local ParserBudget* xml_budget = nullptr;
void* xml_allocate(const std::size_t size) {
    if (xml_budget == nullptr || !xml_budget->reserve_allocation(size)) { return nullptr; }
    return std::malloc(size);
}
void xml_free(void* data) { std::free(data); }
}

XmlBudgetScope::XmlBudgetScope(ParserBudget& budget) : previous_(xml_budget) {
    static std::once_flag initialized;
    std::call_once(initialized, [] { pugi::set_memory_management_functions(xml_allocate, xml_free); });
    xml_budget = &budget;
}
XmlBudgetScope::~XmlBudgetScope() { xml_budget = previous_; }

bool valid_xml_string(const std::string_view value) {
    for (std::size_t at = 0; at < value.size();) {
        const auto first = static_cast<unsigned char>(value[at++]);
        unsigned count = 0;
        std::uint32_t code = first, minimum = 0;
        if (first <= 0x7f) {}
        else if (first >= 0xc2 && first <= 0xdf) { count = 1; code = first & 0x1fU; minimum = 0x80; }
        else if (first >= 0xe0 && first <= 0xef) { count = 2; code = first & 0x0fU; minimum = 0x800; }
        else if (first >= 0xf0 && first <= 0xf4) { count = 3; code = first & 0x07U; minimum = 0x10000; }
        else { return false; }
        if (count > value.size() - at) { return false; }
        for (unsigned index = 0; index < count; ++index) {
            const auto next = static_cast<unsigned char>(value[at++]);
            if ((next & 0xc0U) != 0x80U) { return false; }
            code = (code << 6U) | (next & 0x3fU);
        }
        if (code < minimum || code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff) ||
            code == 0xfffe || code == 0xffff || (code < 0x20 && code != 9 && code != 10 && code != 13)) { return false; }
    }
    return true;
}

std::string expanded_xml_name(pugi::xml_node node, const std::string& name, const bool attribute) {
    const auto colon = name.find(':');
    if (colon == std::string::npos && attribute) { return name; }
    const auto prefix = colon == std::string::npos ? "xmlns" : "xmlns:" + name.substr(0, colon);
    for (auto scope = node; scope; scope = scope.parent()) {
        if (const auto uri = scope.attribute(prefix.c_str())) {
            return "{" + std::string(uri.value()) + "}" + (colon == std::string::npos ? name : name.substr(colon + 1));
        }
    }
    return name;
}

void inspect_xml_fields(Reader& reader, const std::span<const std::uint8_t> bytes, const std::string& packet) {
    XmlBudgetScope scope(reader.context.budget());
    pugi::xml_document document;
    const auto parsed = document.load_buffer(bytes.data(), bytes.size(), pugi::parse_full, pugi::encoding_auto);
    if (!parsed) { throw Failure{parsed.status == pugi::status_out_of_memory ? ParseStatus::AllocationFailure : ParseStatus::Malformed, "Invalid metadata XML", 0}; }
    auto node = document.first_child(); std::uint32_t depth = 0; std::uint64_t index = 0;
    while (node) {
        reader.entry();
        if (!reader.context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "Metadata XML depth limit", 0}; }
        auto field = reader; field.group += ".xml" + std::to_string(index++);
        if (!valid_xml_string(node.name()) || !valid_xml_string(node.value())) { throw Failure{ParseStatus::Malformed, "Invalid metadata XML encoding", 0}; }
        field.derived("depth", depth);
        if (node.type() == pugi::node_element) {
            field.decoded("element", ValueKind::Text, expanded_xml_name(node, node.name()), packet);
            for (const auto attribute : node.attributes()) {
                field.entry(); const auto name = expanded_xml_name(node, attribute.name(), true);
                if (!valid_xml_string(name) || !valid_xml_string(attribute.value())) { throw Failure{ParseStatus::Malformed, "Invalid metadata XML attribute", 0}; }
                field.decoded(name, ValueKind::Text, attribute.value(), packet);
            }
        } else {
            field.decoded("value", ValueKind::Text, node.value(), packet);
            if (node.type() == pugi::node_doctype) { field.diagnostic("doctype", "DTD retained without external or custom entity expansion", 0); }
        }
        if (node.first_child()) { node = node.first_child(); ++depth; }
        else { while (node && !node.next_sibling()) { node = node.parent(); if (depth) { --depth; } } if (node) { node = node.next_sibling(); } }
    }
}

}
