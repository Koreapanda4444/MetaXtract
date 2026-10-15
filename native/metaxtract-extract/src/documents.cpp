#include "zip.hpp"
#include "xml.hpp"

#include <set>
#include <map>

namespace metaxtract::native::formats {
namespace {

std::string local_name(const std::string_view name) {
    const auto colon = name.find(':');
    return std::string(name.substr(colon == std::string_view::npos ? 0 : colon + 1));
}

bool revision_node(const pugi::xml_node node) {
    const auto expanded = expanded_xml_name(node, node.name());
    const auto local = local_name(node.name());
    if (expanded.starts_with("{http://schemas.openxmlformats.org/wordprocessingml/2006/main}") ||
        expanded.starts_with("{http://purl.oclc.org/ooxml/wordprocessingml/main}")) {
        return local == "ins" || local == "del" || local == "moveFrom" || local == "moveTo" ||
            local.ends_with("PrChange") || local == "cellIns" || local == "cellDel" || local == "cellMerge";
    }
    return (expanded.starts_with("{urn:oasis:names:tc:opendocument:xmlns:text:1.0}") && local == "changed-region") ||
        (expanded.starts_with("{http://schemas.openxmlformats.org/spreadsheetml/2006/main}") &&
            (local == "rcc" || local == "rrc" || local == "rm" || local == "rcft"));
}

bool comment_node(const pugi::xml_node node) {
    const auto expanded = expanded_xml_name(node, node.name());
    const auto local = local_name(node.name());
    return (local == "comment" && (expanded.starts_with("{http://schemas.openxmlformats.org/") ||
        expanded.starts_with("{http://purl.oclc.org/ooxml/"))) ||
        expanded == "{urn:oasis:names:tc:opendocument:xmlns:office:1.0}annotation";
}

bool body_part(const std::string& name) {
    return name == "word/document.xml" || name == "content.xml" ||
        name.starts_with("xl/worksheets/") || name.starts_with("ppt/slides/slide");
}

bool embedded_part(const std::string& name) {
    return name.find("/embeddings/") != std::string::npos || name.ends_with("vbaProject.bin") ||
        name.starts_with("Object ") || name.starts_with("Objects/") || name.starts_with("docProps/thumbnail.");
}

void xml_part(Reader& reader, const std::span<const std::uint8_t> bytes, const std::string& packet,
              const std::string& part, std::string& main_type, bool preview) {
    XmlBudgetScope scope(reader.context.budget());
    pugi::xml_document document;
    const auto parsed = document.load_buffer(bytes.data(), bytes.size(),
        pugi::parse_default | pugi::parse_doctype | pugi::parse_comments | pugi::parse_pi, pugi::encoding_auto);
    if (!parsed || !document.document_element()) {
        throw Failure{ParseStatus::Malformed, "Invalid package XML: " + std::string(parsed.description()), reader.base};
    }
    if (part == "[Content_Types].xml") {
        const auto root_name = expanded_xml_name(document.document_element(), document.document_element().name());
        if (root_name != "{http://schemas.openxmlformats.org/package/2006/content-types}Types" &&
            root_name != "{http://purl.oclc.org/ooxml/package/content-types}Types") {
            throw Failure{ParseStatus::Malformed, "Package content types have an unexpected root namespace", 0};
        }
    }
    auto node = document.first_child();
    std::uint32_t depth = 1;
    std::uint64_t sequence = 0, revisions = 0, comments = 0, relationships = 0;
    const bool body = body_part(part);
    while (node) {
        reader.entry(); reader.checkpoint();
        if (!reader.context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "Package XML depth budget exceeded", 0}; }
        Reader field(reader.context, bytes, reader.group + ".node" + std::to_string(sequence++), packet);
        if ((node.type() == pugi::node_pcdata || node.type() == pugi::node_cdata) && !valid_xml_string(node.value())) {
            throw Failure{ParseStatus::Malformed, "Invalid XML text encoding", 0};
        }
        if (node.type() == pugi::node_doctype) {
            field.diagnostic("doctype", "DTD retained; custom and external entities are not expanded", 0);
        } else if (node.type() == pugi::node_element) {
            const auto name = expanded_xml_name(node, node.name());
            if (!valid_xml_string(name)) { throw Failure{ParseStatus::Malformed, "Invalid XML name encoding", 0}; }
            field.decoded("element", ValueKind::Text, name, packet);
            for (const auto attribute : node.attributes()) {
                field.entry();
                const auto attribute_name = expanded_xml_name(node, attribute.name(), true);
                if (!valid_xml_string(attribute_name) || !valid_xml_string(attribute.value())) {
                    throw Failure{ParseStatus::Malformed, "Invalid XML attribute encoding", 0};
                }
                field.decoded(attribute_name, ValueKind::Text, attribute.value(), packet);
            }
            bool text_selected = !body || preview;
            for (auto parent = node; body && parent; parent = parent.parent()) {
                if (revision_node(parent) || comment_node(parent)) { text_selected = true; break; }
            }
            if (text_selected) {
                for (const auto child : node.children()) {
                    if (child.type() == pugi::node_pcdata || child.type() == pugi::node_cdata) {
                        if (!valid_xml_string(child.value())) { throw Failure{ParseStatus::Malformed, "Invalid XML text encoding", 0}; }
                        field.decoded(preview && body ? "preview_text" : name, ValueKind::Text, child.value(), packet);
                    }
                }
            }
            if (revision_node(node)) {
                ++revisions; field.decoded("revision_kind", ValueKind::Text, local_name(node.name()), packet);
            }
            if (comment_node(node)) { ++comments; }
            if (name == "{http://schemas.openxmlformats.org/package/2006/relationships}Relationship" ||
                name == "{http://purl.oclc.org/ooxml/package/relationships}Relationship") {
                ++relationships;
                field.derived("external_relationship", std::string_view(node.attribute("TargetMode").value()) == "External" ? 1 : 0);
            }
            if (part == "[Content_Types].xml" && (name == "{http://schemas.openxmlformats.org/package/2006/content-types}Override" ||
                name == "{http://purl.oclc.org/ooxml/package/content-types}Override")) {
                const std::string type = node.attribute("ContentType").value();
                if (type.ends_with(".main+xml")) {
                    if (type.find("wordprocessingml.") != std::string::npos || type.starts_with("application/vnd.ms-word.")) { main_type = "word"; }
                    else if (type.find("spreadsheetml.") != std::string::npos || type.starts_with("application/vnd.ms-excel.")) { main_type = "spreadsheet"; }
                    else if (type.find("presentationml.") != std::string::npos || type.starts_with("application/vnd.ms-powerpoint.")) { main_type = "presentation"; }
                }
            }
        } else if (node.type() == pugi::node_pi || node.type() == pugi::node_comment) {
            if (!valid_xml_string(node.value())) { throw Failure{ParseStatus::Malformed, "Invalid XML comment/instruction encoding", 0}; }
            field.decoded(node.type() == pugi::node_pi ? "processing_instruction" : "xml_comment", ValueKind::Text, node.value(), packet);
        }
        if (node.first_child()) { node = node.first_child(); ++depth; }
        else {
            while (node && !node.next_sibling()) { node = node.parent(); if (depth > 0) { --depth; } }
            if (node) { node = node.next_sibling(); }
        }
    }
    reader.derived("revision_marker_count", revisions);
    reader.derived("comment_count", comments);
    reader.derived("relationship_count", relationships);
}

ParseStatus package(ParserContext& context, bool preview) {
    Reader reader(context, 0, context.source_size(), "document.package");
    try {
        const auto header = reader.read(0, std::min<std::uint64_t>(reader.size, 4));
        if (!starts(header, "PK")) { return ParseStatus::Unsupported; }
        const auto entries = zip_directory(reader);
        const bool ooxml = std::any_of(entries.begin(), entries.end(), [](const auto& entry) { return entry.name == "[Content_Types].xml"; });
        const auto mime = std::find_if(entries.begin(), entries.end(), [](const auto& entry) { return entry.name == "mimetype"; });
        std::string mimetype;
        if (mime != entries.end()) {
            const auto bytes = zip_payload(reader, *mime);
            mimetype.assign(bytes.begin(), bytes.end());
            reader.decoded("mimetype_payload", ValueKind::DecodedBytes, mimetype, "zip.entry" + std::to_string(mime->index), 3, "binary");
            if (std::any_of(mimetype.begin(), mimetype.end(), [](const auto c) { return static_cast<unsigned char>(c) < 0x21 || static_cast<unsigned char>(c) > 0x7e; })) {
                throw Failure{ParseStatus::Malformed, "Package mimetype is not an ASCII media type", mime->data_offset};
            }
            reader.decoded("mimetype", ValueKind::Text, mimetype, "zip.entry" + std::to_string(mime->index), 3, "ASCII");
        }
        const bool odf = mimetype.starts_with("application/vnd.oasis.opendocument.");
        if (!ooxml && !odf) { return ParseStatus::Unsupported; }
        if (ooxml && odf) { reader.diagnostic("conflicting_family", "Both OOXML content types and an ODF mimetype are present", 0); }
        auto status = ooxml && odf ? ParseStatus::Malformed : ParseStatus::Complete;
        std::set<std::string> encrypted_members;
        std::map<std::uint64_t, std::vector<std::uint8_t>> manifest_payloads;
        if (odf) {
            for (const auto& entry : entries) {
                if (entry.name != "META-INF/manifest.xml") { continue; }
                try {
                    auto bytes = zip_payload(reader, entry);
                    XmlBudgetScope scope(context.budget());
                    pugi::xml_document manifest;
                    const auto parsed = manifest.load_buffer(bytes.data(), bytes.size(), pugi::parse_default | pugi::parse_doctype, pugi::encoding_auto);
                    constexpr std::string_view uri = "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0";
                    if (!parsed || expanded_xml_name(manifest.document_element(), manifest.document_element().name()) != "{" + std::string(uri) + "}manifest") {
                        throw Failure{ParseStatus::Malformed, "ODF manifest has invalid XML or namespace", entry.data_offset};
                    }
                    auto node = manifest.document_element();
                    std::uint32_t depth = 1;
                    while (node) {
                        reader.entry(); reader.checkpoint();
                        if (!context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "ODF manifest depth budget exceeded", entry.data_offset}; }
                        if (expanded_xml_name(node, node.name()) == "{" + std::string(uri) + "}file-entry") {
                            std::string name;
                            for (const auto attribute : node.attributes()) {
                                if (expanded_xml_name(node, attribute.name(), true) == "{" + std::string(uri) + "}full-path") { name = attribute.value(); }
                            }
                            for (const auto child : node.children()) {
                                if (expanded_xml_name(child, child.name()) == "{" + std::string(uri) + "}encryption-data") {
                                    if (!valid_xml_string(name) || !context.budget().reserve_allocation(8 * (name.size() + 64))) {
                                        throw Failure{ParseStatus::Malformed, "ODF encryption member is invalid or exceeds budget", entry.data_offset};
                                    }
                                    encrypted_members.insert(name);
                                }
                            }
                        }
                        if (node.first_child()) { node = node.first_child(); ++depth; }
                        else {
                            while (node && !node.next_sibling()) { node = node.parent(); if (depth > 1) { --depth; } }
                            if (node) { node = node.next_sibling(); }
                        }
                    }
                    if (!context.budget().reserve_allocation(256)) { throw Failure{ParseStatus::Internal, "Manifest cache exceeds budget", 0}; }
                    manifest_payloads.emplace(entry.index, std::move(bytes));
                } catch (const Failure& error) {
                    reader.diagnostic("manifest_failure", error.message, entry.data_offset);
                    if (context.budget().status() != BudgetStatus::Ok) { throw; }
                    status = error.status;
                }
            }
        }
        std::set<std::string> seen;
        std::string main_type;
        std::uint64_t macros = 0, signatures = 0, embedded = 0;
        for (const auto& entry : entries) {
            reader.checkpoint();
            if (!seen.insert(entry.name).second || unsafe_member_name(entry.name)) {
                reader.diagnostic("ambiguous_member", "Duplicate or non-canonical package member retained by entry index", entry.central_offset);
                status = ParseStatus::Malformed;
            }
            const bool xml = entry.name.ends_with(".xml") || entry.name.ends_with(".rels");
            const bool attachment = embedded_part(entry.name);
            macros += entry.name.ends_with("vbaProject.bin") ? 1 : 0;
            signatures += entry.name.starts_with("_xmlsignatures/") || entry.name == "META-INF/documentsignatures.xml" ? 1 : 0;
            embedded += attachment ? 1 : 0;
            if (!xml && !attachment) { continue; }
            const auto packet = "zip.entry" + std::to_string(entry.index) + "/" + path_component(entry.name);
            if (encrypted_members.contains(entry.name)) {
                auto raw = reader.window(entry.data_offset, entry.compressed_size, "document.package.part" + std::to_string(entry.index) + ".protected");
                raw.put("encrypted_payload", ValueKind::Bytes, "", 0, raw.read(0, raw.size), "binary");
                raw.decoded("encrypted_member", ValueKind::Text, entry.name, packet);
                raw.derived("password_required", 1);
                if (status == ParseStatus::Complete) { status = ParseStatus::Unsupported; }
                continue;
            }
            try {
                const auto cached = manifest_payloads.find(entry.index);
                const auto bytes = cached == manifest_payloads.end() ? zip_payload(reader, entry) : std::move(cached->second);
                Reader part(context, bytes, "document.package.part" + std::to_string(entry.index), packet);
                part.decoded("part_name", ValueKind::DecodedBytes, entry.name, packet, 3, (entry.flags & 0x800U) ? "UTF-8" : "CP437");
                part.decoded("part_payload", ValueKind::DecodedBytes, {bytes.begin(), bytes.end()}, packet, 3, "binary");
                part.derived("crc_verified", 1);
                if (xml) { xml_part(part, bytes, packet, entry.name, main_type, preview); }
            } catch (const Failure& error) {
                Reader failure(context, std::span<const std::uint8_t>{}, "diagnostic.document.package.part" + std::to_string(entry.index), packet);
                failure.diagnostic("part_failure", path_component(entry.name) + ": " + error.message, 0);
                if (context.budget().status() != BudgetStatus::Ok) { throw; }
                if (status == ParseStatus::Complete || error.status == ParseStatus::Malformed) { status = error.status; }
            }
        }
        if (!main_type.empty()) { reader.decoded("declared_main_type", ValueKind::Text, main_type, "package", 0); }
        reader.decoded("package_family", ValueKind::Text, !main_type.empty() ? "OOXML" : odf ? "ODF" : "OPC", "package", 0);
        reader.derived("macro_part_count", macros); reader.derived("signature_part_count", signatures);
        reader.derived("embedded_part_count", embedded);
        if (signatures != 0) { reader.decoded("signature_verification", ValueKind::Text, "not_performed", "package", 0); }
        return status;
    } catch (const Failure& error) {
        reader.diagnostic("failure", error.message, error.offset >= reader.base ? error.offset - reader.base : 0);
        return error.status;
    }
}

}
}

namespace metaxtract::native {
namespace {
class PackageParser final : public Parser {
    bool preview_;
public:
    explicit PackageParser(bool preview=false):preview_(preview){}
    const ParserCapability& capability() const noexcept override {
        static const ParserCapability value{"document.package", "OOXML and ODF package metadata", "1", Extract | EnumerateChildren,
            {"docx", "docm", "dotx", "dotm", "xlsx", "xlsm", "xltx", "xltm", "pptx", "pptm", "potx", "potm", "ppsx", "ppsm", "odt", "ods", "odp", "odg", "odf", "ott", "ots", "otp"}};
        static const ParserCapability preview{"document.preview","Bounded OOXML and ODF body text preview","1",Extract | DecodePreview,value.formats};
        return preview_?preview:value;
    }
    ParseStatus run(ParserContext& context) const override { return formats::package(context,preview_); }
};
}
std::unique_ptr<Parser> document_package_parser() { return std::make_unique<PackageParser>(); }
std::unique_ptr<Parser> document_preview_parser() { return std::make_unique<PackageParser>(true); }
}
