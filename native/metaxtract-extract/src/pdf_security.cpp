#include "pdf.hpp"
#include <climits>
#include <qpdf/QPDFXRefEntry.hh>

namespace metaxtract::native::formats::pdf {
namespace {
bool white(unsigned char c) { return c == 0 || c == 9 || c == 10 || c == 12 || c == 13 || c == 32; }
int nibble(unsigned char c) {
    if (c >= '0' && c <= '9') { return c - '0'; }
    if (c >= 'a' && c <= 'f') { return c - 'a' + 10; }
    if (c >= 'A' && c <= 'F') { return c - 'A' + 10; }
    return -1;
}
std::string method(QPDF::encryption_method_e type) {
    switch (type) {
    case QPDF::e_none: return "none";
    case QPDF::e_rc4: return "RC4";
    case QPDF::e_aes: return "AES-128";
    case QPDF::e_aesv3: return "AES-256";
    default: return "unknown";
    }
}
class Objects final {
    Reader& source_;
    unsigned signatures_ = 0;
    std::uint64_t features_ = 0;
    std::uint64_t active_features_ = 0;
    std::set<QPDFObjGen> scripts_;
    bool original_offsets_ = false;
    void feature(Reader& r, const std::string& name, const std::string& id, bool active = true) {
        ++features_; value(r, "security_structure_feature", ValueKind::Text, name, id);
        if (active) { ++active_features_; value(r, "active_content_feature", ValueKind::Text, name, id); }
    }
    void script(Reader& r, QPDFObjectHandle data, const std::string& id) {
        if (data.isString()) { value(r, "javascript_data", ValueKind::DecodedBytes, data.getStringValue(), id); return; }
        if (!data.isStream()) { value(r, "javascript_data_status", ValueKind::Text, "not_a_string_or_stream", id); return; }
        if (data.isIndirect() && !scripts_.insert(data.getObjGen()).second) { return; }
        if (!data.getDict().getKey("/F").isNull()) {
            value(r, "javascript_data_status", ValueKind::Text, "external_stream_not_opened", id); return;
        }
        auto data_bytes = stream_bytes(source_, data, true);
        value(r, "javascript_data", ValueKind::DecodedBytes, std::string(data_bytes.begin(), data_bytes.end()), object_id(data));
    }
    void signature(QPDFObjectHandle dictionary, const std::string& id) {
        Reader r(source_.context, 0, source_.size, "pdf.signature" + std::to_string(signatures_++));
        value(r, "signature_object", ValueKind::Text, id, id);
        value(r, "cryptographic_verification", ValueKind::Text, "not_performed", id);
        auto contents = dictionary.getKey("/Contents");
        if (contents.isString()) { value(r, "signature_contents", ValueKind::DecodedBytes, contents.getStringValue(), id); }
        for (const auto* key : {"/Filter", "/SubFilter", "/Name", "/Reason", "/Location", "/M", "/ContactInfo", "/Cert", "/Reference", "/Prop_Build"}) {
            auto item = dictionary.getKey(key);
            if (!item.isNull()) { scalar_tree(r, item, std::string(key).substr(1), id); }
        }
        auto ranges = dictionary.getKey("/ByteRange");
        if (!ranges.isArray()) {
            number(r, "byte_range_valid", 0, id); value(r, "byte_range_issue", ValueKind::Text, "missing_or_non_array", id); return;
        }
        const auto count = ranges.getArrayNItems();
        number(r, "byte_range_items", count, id);
        bool valid = count >= 4 && count % 2 == 0;
        std::uint64_t last = 0, first_end = 0, second_begin = 0;
        for (int i = 0; i < count; ++i) {
            r.entry(); auto item = ranges.getArrayItem(i);
            if (!item.isInteger()) { valid = false; continue; }
            const auto n = item.getIntValue();
            number(r, "byte_range." + std::to_string(i), n, id);
            if (n < 0) { valid = false; }
        }
        if (valid) {
            for (int i = 0; i < count; i += 2) {
                const auto begin = static_cast<std::uint64_t>(ranges.getArrayItem(i).getIntValue());
                const auto length = static_cast<std::uint64_t>(ranges.getArrayItem(i + 1).getIntValue());
                if ((i == 0 && begin != 0) || begin < last || begin > source_.size || length > source_.size - begin) { valid = false; break; }
                last = begin + length;
                if (i == 0) { first_end = last; }
                if (i == 2) { second_begin = begin; }
            }
        }
        number(r, "byte_range_valid", valid ? 1 : 0, id);
        if (!valid) { value(r, "byte_range_issue", ValueKind::Text, "invalid_count_type_order_or_source_bounds", id); return; }
        number(r, "signed_covered_end", static_cast<std::int64_t>(last), id);
        number(r, "unsigned_tail_bytes", static_cast<std::int64_t>(source_.size - last), id);
        number(r, "covered_end_is_current_eof", last == source_.size ? 1 : 0, id);
        const auto parsed_offset = contents.isString() && !contents.isIndirect() ? contents.getParsedOffset() : -1;
        const bool can_check = count == 4 && contents.isString() && !contents.isIndirect() && original_offsets_ && parsed_offset >= 0;
        number(r, "contents_gap_checked", can_check ? 1 : 0, id);
        if (!can_check) { return; }
        number(r, "signature_contents_source_offset", parsed_offset, id);
        const auto gap = source_.read(first_end, second_begin - first_end);
        bool match = static_cast<std::uint64_t>(parsed_offset) == first_end && gap.size() >= 2 && gap.front() == '<' && gap.back() == '>';
        const auto expected = contents.getStringValue();
        std::size_t index = 0; int high = -1;
        for (std::size_t i = 1; match && i + 1 < gap.size(); ++i) {
            if (white(gap[i])) { continue; }
            const auto digit = nibble(gap[i]);
            if (digit < 0) { match = false; break; }
            if (high < 0) { high = digit; }
            else {
                if (index >= expected.size() || static_cast<unsigned char>(expected[index++]) != static_cast<unsigned char>((high << 4) | digit)) { match = false; }
                high = -1;
            }
        }
        if (match && high >= 0) {
            if (index >= expected.size() || static_cast<unsigned char>(expected[index++]) != static_cast<unsigned char>(high << 4)) { match = false; }
        }
        match = match && index == expected.size();
        number(r, "contents_gap_matches", match ? 1 : 0, id);
        value(r, "contents_gap_check_scope", ValueKind::Text, "source_offset_and_hex_string_bytes_not_signature_authentication", id);
    }
    void walk(Reader& r, QPDFObjectHandle object, const std::string& key, const std::string& id, std::uint32_t level, bool definition = false) {
        r.entry(); depth(r, level);
        if (object.isIndirect() && !definition) {
            value(r, key + ".reference", ValueKind::Text, object_id(object), id); return;
        }
        if (object.isArray()) {
            number(r, key + ".items", object.getArrayNItems(), id);
            for (int i = 0; i < object.getArrayNItems(); ++i) { walk(r, object.getArrayItem(i), key + "." + std::to_string(i), id, level + 1); }
            return;
        }
        if (object.isDictionary() || object.isStream()) {
            auto dictionary = object.isStream() ? object.getDict() : object;
            auto type = dictionary.getKey("/Type");
            if ((type.isName() && type.getName() == "/Sig") || dictionary.hasKey("/ByteRange")) { signature(dictionary, id); }
            auto field = dictionary.getKey("/FT");
            if (field.isName() && field.getName() == "/Sig") {
                number(r, "signature_field", 1, id);
                number(r, "signature_field_has_value", dictionary.getKey("/V").isNull() ? 0 : 1, id);
            }
            if (type.isName()) {
                const auto name = type.getName();
                if (name == "/EmbeddedFile" || name == "/ObjStm" || name == "/XRef" || name == "/RichMedia") { feature(r, "type:" + name, id, name == "/RichMedia"); }
            }
            auto action = dictionary.getKey("/S");
            if (action.isName()) {
                const auto name = action.getName();
                if (name == "/JavaScript" || name == "/Launch" || name == "/URI" || name == "/GoToR" || name == "/GoToE" ||
                    name == "/SubmitForm" || name == "/ImportData" || name == "/Rendition") { feature(r, "action:" + name, id); }
            }
            for (const auto& name : dictionary.getKeys()) {
                r.entry(); value(r, "object_dictionary_key", ValueKind::Text, name, id);
                if (name == "/OpenAction" || name == "/AA" || name == "/JS" || name == "/JavaScript" || name == "/Launch" ||
                    name == "/RichMedia" || name == "/RichMediaContent" || name == "/XFA" || name == "/EmbeddedFiles" ||
                    name == "/URI" || name == "/EF") { feature(r, "key:" + name, id, name != "/EmbeddedFiles" && name != "/EF"); }
                auto item = dictionary.getKey(name);
                if (name == "/JS") { script(r, item, id); }
                if (object.isStream() && name == "/F") { value(r, "external_stream_not_opened", ValueKind::Text, "Filesystem access is disabled", id); }
                walk(r, item, key.empty() ? path_component(name.substr(1)) : key + "." + path_component(name.substr(1)), id, level + 1);
            }
            if (object.isStream()) { value(r, "stream_payload_status", ValueKind::Text, "not_decoded_by_object_inventory", id); }
            return;
        }
        if (object.isString()) {
            value(r, key + ".string_bytes", ValueKind::DecodedBytes, object.getStringValue(), id);
            value(r, key, ValueKind::Text, object.getUTF8Value(), id);
        } else if (object.isName()) { value(r, key, ValueKind::Text, object.getName(), id); }
        else if (object.isInteger()) { number(r, key, object.getIntValue(), id); }
        else if (object.isReal()) { value(r, key, ValueKind::Float, object.getRealValue(), id); }
        else if (object.isBool()) { number(r, key, object.getBoolValue() ? 1 : 0, id); }
        else { value(r, key + ".type", ValueKind::Text, object.getTypeName(), id); }
    }
public:
    explicit Objects(Reader& r) : source_(r) {}
    void dictionary(Reader& r, QPDFObjectHandle dictionary, const std::string& key, const std::string& id) { walk(r, dictionary, key, id, 0, true); }
    void inventory(QPDF& document, QPDF& raw, const History& revisions) {
        constexpr std::uint64_t entry_allowance = 96;
        const auto known = static_cast<std::uint64_t>(revisions.slots.size());
        if (known > UINT64_MAX / entry_allowance || !source_.context.budget().reserve_allocation(known * entry_allowance)) {
            throw Failure{ParseStatus::Internal, "PDF object inventory allocation limit exceeded", 0};
        }
        source_.checkpoint();
        const auto entries = document.getXRefTable();
        source_.checkpoint();
        if (entries.size() > known && !source_.context.budget().reserve_allocation((entries.size() - known) * entry_allowance)) {
            throw Failure{ParseStatus::Internal, "PDF library object inventory allocation limit exceeded", 0};
        }
        value(source_, "object_inventory_source", ValueKind::Text, "qpdf_current_cross_reference_table", "pdf.container");
        std::uint64_t count = 0;
        for (const auto& [identity, slot] : entries) {
            source_.entry(); source_.checkpoint();
            const auto type = slot.getType();
            if (type != 1 && type != 2) { continue; }
            if (identity.getObj() < 0 || identity.getGen() < 0) { throw Failure{ParseStatus::Malformed, "PDF object identity is negative", 0}; }
            auto object = document.getObject(identity);
            Reader r(source_.context, 0, source_.size, "pdf.object" + std::to_string(identity.getObj()) + "." + std::to_string(identity.getGen()));
            const auto id = object_id(object);
            number(r, "xref_type", type, id);
            if (type == 2) {
                number(r, "object_stream", slot.getObjStreamNumber(), id);
                number(r, "object_stream_index", slot.getObjStreamIndex(), id);
            } else {
                const auto offset = slot.getOffset();
                if (offset < 0 || static_cast<std::uint64_t>(offset) >= source_.size) { throw Failure{ParseStatus::Malformed, "PDF library object offset escapes source", 0}; }
                number(r, "object_offset", offset, id);
                if (object.isDictionary() || object.isStream()) {
                    static_cast<void>(raw_dictionary(r, raw, static_cast<std::uint64_t>(offset), static_cast<std::uint64_t>(identity.getObj()), static_cast<std::uint64_t>(identity.getGen())));
                }
            }
            original_offsets_ = type == 1;
            dictionary(r, object, "", id); ++count;
        }
        number(source_, "inspected_object_count", static_cast<std::int64_t>(count), "pdf.container");
        number(source_, "signature_count", signatures_, "pdf.container");
        number(source_, "security_structure_feature_count", static_cast<std::int64_t>(features_), "pdf.container");
        number(source_, "active_content_feature_count", static_cast<std::int64_t>(active_features_), "pdf.container");
    }
};
void encryption(QPDF* document, QPDF& raw, Reader& source, const History& revisions, Objects& objects) {
    Reader r(source.context, 0, source.size, "pdf.encryption");
    QPDFObjectHandle dictionary = QPDFObjectHandle::newNull();
    if (document) { dictionary = document->getTrailer().getKey("/Encrypt"); }
    else if (!revisions.revisions.empty()) {
        dictionary = revisions.revisions.front().trailer.getKey("/Encrypt");
        if (dictionary.isIndirect()) {
            const auto id = dictionary.getObjGen(); const auto found = revisions.slots.find(static_cast<std::uint64_t>(id.getObj()));
            value(r, "encryption_dictionary_reference", ValueKind::Text, object_id(dictionary), "pdf.container");
            if (found == revisions.slots.end() || found->second.type != 1 || found->second.generation != static_cast<std::uint64_t>(id.getGen())) {
                value(r, "encryption_dictionary_status", ValueKind::Text, "raw_dictionary_not_resolved", "pdf.container"); return;
            }
            dictionary = raw_dictionary(source, raw, found->second.offset, static_cast<std::uint64_t>(id.getObj()), found->second.generation);
        }
    }
    if (!dictionary.isNull()) {
        if (!dictionary.isDictionary()) { throw Failure{ParseStatus::Malformed, "PDF encryption value is not a dictionary", 0}; }
        objects.dictionary(r, dictionary, "encryption", "pdf.encryption");
    }
    if (!document || !document->isEncrypted()) { return; }
    int revision = 0, permissions = 0, version = 0;
    QPDF::encryption_method_e streams = QPDF::e_unknown, strings = QPDF::e_unknown, files = QPDF::e_unknown;
    static_cast<void>(document->isEncrypted(revision, permissions, version, streams, strings, files));
    number(r, "encryption_revision", revision, "pdf.encryption");
    number(r, "encryption_version", version, "pdf.encryption");
    number(r, "encryption_permissions", permissions, "pdf.encryption");
    value(r, "stream_encryption", ValueKind::Text, method(streams), "pdf.encryption");
    value(r, "string_encryption", ValueKind::Text, method(strings), "pdf.encryption");
    value(r, "attachment_encryption", ValueKind::Text, method(files), "pdf.encryption");
    number(r, "empty_user_password_matched", document->userPasswordMatched() ? 1 : 0, "pdf.encryption");
    number(r, "empty_owner_password_matched", document->ownerPasswordMatched() ? 1 : 0, "pdf.encryption");
    for (const auto& [key, allowed] : std::initializer_list<std::pair<const char*, bool>>{
        {"permission.accessibility", document->allowAccessibility()}, {"permission.extract", document->allowExtractAll()},
        {"permission.print_low", document->allowPrintLowRes()}, {"permission.print_high", document->allowPrintHighRes()},
        {"permission.assemble", document->allowModifyAssembly()}, {"permission.forms", document->allowModifyForm()},
        {"permission.annotate", document->allowModifyAnnotation()}, {"permission.modify_other", document->allowModifyOther()},
        {"permission.modify_all", document->allowModifyAll()}}) {
        number(r, key, allowed ? 1 : 0, "pdf.encryption");
    }
}
void security(QPDF* document, QPDF& raw, Reader& source, const History& revisions) {
    Reader r(source.context, 0, source.size, "pdf.security");
    std::uint64_t forward_links = 0;
    for (const auto& revision : revisions.revisions) {
        auto previous = revision.trailer.getKey("/Prev");
        if (!previous.isIndirect() && previous.isInteger() && previous.getIntValue() >= 0 &&
            static_cast<std::uint64_t>(previous.getIntValue()) > revision.offset) { ++forward_links; }
    }
    const auto links = revisions.revisions.empty() ? 0 : revisions.revisions.size() - 1;
    number(r, "forward_xref_link_count", static_cast<std::int64_t>(forward_links), "pdf.container");
    number(r, "incremental_update_count", static_cast<std::int64_t>(links - std::min<std::uint64_t>(links, forward_links)), "pdf.container");
    value(r, "incremental_count_scope", ValueKind::Text, "xref_chain_minus_forward_links_not_full_historical_object_reconstruction", "pdf.container");
    if (document) { number(r, "linearization_header_present", document->isLinearized() ? 1 : 0, "pdf.container"); }
    number(r, "latest_eof_end", static_cast<std::int64_t>(revisions.eof_end), "pdf.container");
    std::uint64_t nonwhite = 0;
    for (auto at = revisions.eof_end; at < source.size;) {
        const auto data = source.read(at, std::min<std::uint64_t>(65536, source.size - at));
        for (auto c : data) { if (!white(c)) { ++nonwhite; } }
        at += data.size();
    }
    number(r, "trailing_non_whitespace_bytes", static_cast<std::int64_t>(nonwhite), "pdf.container");
    value(r, "active_content_execution", ValueKind::Text, "disabled", "pdf.container");
    Objects objects(r); encryption(document, raw, r, revisions, objects);
    if (!document) {
        value(r, "object_inventory_status", ValueKind::Text, "password_required", "pdf.container"); return;
    }
    objects.inventory(*document, raw, revisions);
}
}
ParseStatus inspect_security(ParserContext& context) { return inspect(context, security); }
}
namespace metaxtract::native {
namespace {
class PdfSecurityParser final : public Parser {
public:
    const ParserCapability& capability() const noexcept override {
        static const ParserCapability value{"pdf.security", "Read-only PDF encryption signature ranges and object inventory", "12.4.2", Probe | Extract, {"pdf"}};
        return value;
    }
    ParseStatus run(ParserContext& context) const override { return formats::pdf::inspect_security(context); }
};
}
std::unique_ptr<Parser> pdf_security_parser() { return std::make_unique<PdfSecurityParser>(); }
}
