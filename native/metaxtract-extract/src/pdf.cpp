#include "pdf.hpp"
#include "pdf_budget.hpp"
#include <qpdf/InputSource.hh>
#include <qpdf/QPDFExc.hh>
#include <cmath>
#include <memory>

namespace metaxtract::native::formats::pdf {
namespace {
class Source final : public InputSource {
    Reader& r_;
    qpdf_offset_t position_ = 0;
    const std::string name_ = "MetaXtract Rust-owned PDF source";
public:
    explicit Source(Reader& r) : r_(r) {}
    const std::string& getName() const override { return name_; }
    qpdf_offset_t tell() override { return position_; }
    void seek(qpdf_offset_t offset, int whence) override {
        r_.checkpoint();
        const auto base = whence == SEEK_SET ? 0 : whence == SEEK_CUR ? position_ : whence == SEEK_END ? static_cast<qpdf_offset_t>(r_.size) : -1;
        if (base < 0 || (offset < 0 && offset < -base) || (offset > 0 && offset > INT64_MAX - base)) {
            throw Failure{ParseStatus::OutOfRange, "PDF library seek is invalid", r_.base};
        }
        position_ = base + offset;
    }
    void rewind() override { position_ = 0; }
    void unreadCh(char) override { if (position_ > 0) { --position_; } }
    std::size_t read(char* buffer, std::size_t length) override {
        r_.checkpoint();
        if (static_cast<std::uint64_t>(position_) >= r_.size) { setLastOffset(static_cast<qpdf_offset_t>(r_.size)); return 0; }
        setLastOffset(position_);
        const auto count = std::min<std::uint64_t>(length, r_.size - static_cast<std::uint64_t>(position_));
        std::uint64_t completed = 0;
        while (completed < count) {
            const auto part = r_.read(static_cast<std::uint64_t>(position_), std::min<std::uint64_t>(65536, count - completed));
            std::memcpy(buffer + static_cast<std::size_t>(completed), part.data(), part.size());
            position_ += static_cast<qpdf_offset_t>(part.size()); completed += part.size();
        }
        return static_cast<std::size_t>(completed);
    }
    qpdf_offset_t findAndSkipNextEOL() override {
        char c = 0;
        while (read(&c, 1)) {
            if (c != '\r' && c != '\n') { continue; }
            const auto result = position_ - 1;
            while (read(&c, 1)) { if (c != '\r' && c != '\n') { unreadCh(c); break; } }
            return result;
        }
        return position_;
    }
};
void tree(Reader& r, QPDFObjectHandle object, const std::string& key, const std::string& stream, std::uint32_t level, std::set<QPDFObjGen>& active) {
    r.entry(); depth(r, level);
    auto id = stream; bool indirect = object.isIndirect();
    if (indirect) {
        id = object_id(object); value(r, key + ".reference", ValueKind::Text, id, stream);
        if (!active.insert(object.getObjGen()).second) { value(r, key + ".cycle", ValueKind::Text, id, stream); return; }
    }
    if (object.isString()) {
        value(r, key + ".string_bytes", ValueKind::DecodedBytes, object.getStringValue(), id);
        value(r, key, ValueKind::Text, object.getUTF8Value(), id);
    } else if (object.isName()) { value(r, key, ValueKind::Text, object.getName(), id); }
    else if (object.isInteger()) { number(r, key, object.getIntValue(), id); }
    else if (object.isReal()) { value(r, key, ValueKind::Float, object.getRealValue(), id); }
    else if (object.isBool()) { number(r, key, object.getBoolValue() ? 1 : 0, id); }
    else if (object.isNull()) { value(r, key + ".type", ValueKind::Text, "null", id); }
    else if (object.isArray()) {
        number(r, key + ".items", object.getArrayNItems(), id);
        for (int i = 0; i < object.getArrayNItems(); ++i) { tree(r, object.getArrayItem(i), key + "." + std::to_string(i), id, level + 1, active); }
    } else {
        const auto dictionary = object.isStream() ? object.getDict() : object;
        if (dictionary.isDictionary()) {
            for (const auto& name : dictionary.getKeys()) {
                r.entry(); value(r, "dictionary_key", ValueKind::Text, name, id);
                tree(r, dictionary.getKey(name), key.empty() ? path_component(name.substr(1)) : key + "." + path_component(name.substr(1)), id, level + 1, active);
            }
            if (object.isStream()) { value(r, key + ".stream", ValueKind::Text, "Stream data is extracted only for selected metadata/attachments", id); }
        } else { value(r, key + ".type", ValueKind::Text, object.getTypeName(), id); }
    }
    if (indirect) { active.erase(object.getObjGen()); }
}
void packet(Reader& r, QPDFObjectHandle stream, const std::string& key) {
    auto data = stream_bytes(r, stream, true);
    value(r, key, ValueKind::DecodedBytes, std::string(data.begin(), data.end()), object_id(stream));
}
class Attachments final {
    Reader& r_;
    unsigned count_ = 0;
    std::set<QPDFObjGen> nodes_;
public:
    explicit Attachments(Reader& r) : r_(r) {}
    void file(QPDFObjectHandle object, QPDFObjectHandle tree_name, const std::string& association) {
        r_.entry();
        if (!object.isDictionary()) { throw Failure{ParseStatus::Malformed, "PDF attachment file specification is not a dictionary", 0}; }
        Reader r(r_.context, 0, 0, "pdf.attachment" + std::to_string(count_++)); const auto id = object_id(object);
        value(r, "attachment_association", ValueKind::Text, association, id);
        scalar_tree(r, object, "file_spec", id);
        if (tree_name.isString()) { value(r, "attachment_name", ValueKind::Text, tree_name.getUTF8Value(), id); }
        auto filename = object.getKey("/UF"); if (!filename.isString()) { filename = object.getKey("/F"); }
        if (filename.isString()) { value(r, "attachment_filename", ValueKind::Text, filename.getUTF8Value(), id); }
        for (const auto* key : {"/F", "/UF", "/Desc", "/AFRelationship"}) {
            auto field = object.getKey(key); if (!field.isNull()) { scalar_tree(r, field, std::string(key).substr(1), id); }
        }
        auto ef = object.getKey("/EF");
        if (!ef.isDictionary()) { value(r, "external_file_not_opened", ValueKind::Text, "File specification has no embedded data; no filesystem access was attempted", id); return; }
        std::set<QPDFObjGen> emitted;
        for (const auto& key : ef.getKeys()) {
            r.entry(); auto stream = ef.getKey(key);
            value(r, "attachment_stream_key", ValueKind::Text, key, id);
            if (!stream.isStream()) { throw Failure{ParseStatus::Malformed, "PDF embedded-file reference is not a stream", 0}; }
            if (stream.isIndirect() && !emitted.insert(stream.getObjGen()).second) { continue; }
            scalar_tree(r, stream.getDict(), "stream_dictionary", object_id(stream));
            auto subtype = stream.getDict().getKey("/Subtype");
            if (subtype.isName()) { value(r, "attachment_mime", ValueKind::Text, subtype.getName().substr(1), object_id(stream)); }
            scalar_tree(r, stream.getDict().getKey("/Params"), "Params", object_id(stream));
            packet(r, stream, "attachment_data");
        }
    }
    void names(QPDFObjectHandle node, std::uint32_t level = 0) {
        r_.entry(); depth(r_, level);
        if (!node.isDictionary()) { throw Failure{ParseStatus::Malformed, "PDF embedded-file name tree is not a dictionary", 0}; }
        if (node.isIndirect() && !nodes_.insert(node.getObjGen()).second) { throw Failure{ParseStatus::Malformed, "PDF attachment name tree is cyclic", 0}; }
        auto pairs = node.getKey("/Names");
        if (!pairs.isNull()) {
            if (!pairs.isArray() || pairs.getArrayNItems() % 2) { throw Failure{ParseStatus::Malformed, "PDF attachment name tree has invalid pairs", 0}; }
            for (int i = 0; i < pairs.getArrayNItems(); i += 2) {
                auto name = pairs.getArrayItem(i);
                if (!name.isString()) { throw Failure{ParseStatus::Malformed, "PDF attachment name is not a string", 0}; }
                file(pairs.getArrayItem(i + 1), name, "EmbeddedFiles name tree");
            }
        }
        auto kids = node.getKey("/Kids");
        if (!kids.isNull()) {
            if (!kids.isArray()) { throw Failure{ParseStatus::Malformed, "PDF name tree Kids is not an array", 0}; }
            for (int i = 0; i < kids.getArrayNItems(); ++i) { names(kids.getArrayItem(i), level + 1); }
        }
        if (node.isIndirect()) { nodes_.erase(node.getObjGen()); }
    }
    void associated(QPDFObjectHandle array, const std::string& role) {
        if (array.isNull()) { return; }
        if (!array.isArray()) { throw Failure{ParseStatus::Malformed, "PDF associated files value is not an array", 0}; }
        for (int i = 0; i < array.getArrayNItems(); ++i) { file(array.getArrayItem(i), QPDFObjectHandle::newNull(), role); }
    }
    void finish() { number(r_, "attachment_count", count_, "pdf.container"); }
};
struct Inherited {
    QPDFObjectHandle media = QPDFObjectHandle::newNull();
    QPDFObjectHandle crop = QPDFObjectHandle::newNull();
    QPDFObjectHandle rotate = QPDFObjectHandle::newNull();
};
class Pages final {
    Reader& r_; Attachments& attachments_;
    std::set<QPDFObjGen> visited_;
    unsigned count_ = 0;
    void box(Reader& r, const std::string& key, QPDFObjectHandle data, const std::string& id) {
        if (!data.isArray() || data.getArrayNItems() != 4) { throw Failure{ParseStatus::Malformed, "PDF page box is not a four-number array", 0}; }
        std::string values; double coordinates[4]{};
        for (int i = 0; i < 4; ++i) {
            auto v = data.getArrayItem(i);
            if (!v.isNumber() || !std::isfinite(v.getNumericValue())) { throw Failure{ParseStatus::Malformed, "PDF page box contains an invalid number", 0}; }
            coordinates[i] = v.getNumericValue();
            std::ostringstream rendered; rendered << std::setprecision(17) << coordinates[i];
            if (i) { values += ","; } values += rendered.str();
        }
        value(r, key, ValueKind::FloatSeq, values, id);
        number(r, key + ".positive_extent", coordinates[2] > coordinates[0] && coordinates[3] > coordinates[1] ? 1 : 0, id);
    }
public:
    Pages(Reader& r, Attachments& a) : r_(r), attachments_(a) {}
    void walk(QPDFObjectHandle node, Inherited inherited = {}, std::uint32_t level = 0) {
        r_.entry(); depth(r_, level);
        if (!node.isDictionary()) { throw Failure{ParseStatus::Malformed, "PDF page-tree node is not a dictionary", 0}; }
        if (node.isIndirect() && !visited_.insert(node.getObjGen()).second) { throw Failure{ParseStatus::Malformed, "PDF page tree repeats an object", 0}; }
        const auto update = [&](const char* key, QPDFObjectHandle& target) { auto v = node.getKey(key); if (!v.isNull()) { target = v; } };
        update("/MediaBox", inherited.media); update("/CropBox", inherited.crop); update("/Rotate", inherited.rotate);
        auto type = node.getKey("/Type");
        if (type.isName() && type.getName() == "/Pages") {
            auto kids = node.getKey("/Kids");
            if (!kids.isArray()) { throw Failure{ParseStatus::Malformed, "PDF page-tree Kids is not an array", 0}; }
            for (int i = 0; i < kids.getArrayNItems(); ++i) { walk(kids.getArrayItem(i), inherited, level + 1); }
            return;
        }
        if (!type.isName() || type.getName() != "/Page") { throw Failure{ParseStatus::Malformed, "PDF page-tree leaf has no Page type", 0}; }
        Reader r(r_.context, 0, 0, "pdf.page" + std::to_string(count_)); const auto id = object_id(node);
        number(r, "page_number", ++count_, id); value(r, "page_object", ValueKind::Text, id, id);
        box(r, "media_box", inherited.media, id);
        const auto crop = inherited.crop.isNull() ? inherited.media : inherited.crop;
        box(r, "crop_box", crop, id);
        for (const auto* key : {"/BleedBox", "/TrimBox", "/ArtBox"}) {
            auto v = node.getKey(key); box(r, std::string(key).substr(1), v.isNull() ? crop : v, id);
        }
        if (inherited.rotate.isNull()) { number(r, "rotation", 0, id); }
        else if (inherited.rotate.isInteger()) { number(r, "rotation", inherited.rotate.getIntValue(), id); }
        else { throw Failure{ParseStatus::Malformed, "PDF page rotation is not an integer", 0}; }
        auto unit = node.getKey("/UserUnit");
        if (unit.isNull()) { value(r, "user_unit", ValueKind::Float, "1", id); }
        else if (unit.isNumber()) { value(r, "user_unit", ValueKind::Float, unit.unparse(), id); }
        else { throw Failure{ParseStatus::Malformed, "PDF page unit is not numeric", 0}; }
        auto metadata = node.getKey("/Metadata");
        if (!metadata.isNull()) { packet(r, metadata, "page_xmp_packet"); }
        attachments_.associated(node.getKey("/AF"), "Page associated files");
    }
    void finish() { number(r_, "page_count", count_, "pdf.container"); }
};
}
void scalar_tree(Reader& r, QPDFObjectHandle object, const std::string& key, const std::string& stream, std::uint32_t level) {
    std::set<QPDFObjGen> active; tree(r, object, key, stream, level, active);
}
ParseStatus inspect(ParserContext& context, SecurityInspector security) {
    if (context.source_size() < 8 || context.source_size() > static_cast<std::uint64_t>(INT64_MAX)) { return ParseStatus::Unsupported; }
    Reader r(context, 0, context.source_size(), "pdf.document");
    try {
        const auto prefix = r.read(0, std::min<std::uint64_t>(1024, r.size));
        const std::string header(prefix.begin(), prefix.end()); const auto at = header.find("%PDF-");
        if (at == std::string::npos) { return ParseStatus::Unsupported; }
        if (at + 8 > header.size() || header[at + 5] < '0' || header[at + 5] > '9' || header[at + 6] != '.' || header[at + 7] < '0' || header[at + 7] > '9') {
            throw Failure{ParseStatus::Malformed, "PDF header version is invalid", at};
        }
        r.put("header_version", ValueKind::Text, header.substr(at + 5, 3), at + 5, r.read(at + 5, 3), "ASCII");
        if (!context.budget().reserve_allocation(32U * 1024U * 1024U)) { throw Failure{ParseStatus::Internal, "PDF library working allowance exceeded", 0}; }
        BufferBudgetScope buffers(context.budget());
        QPDF raw; raw.setSuppressWarnings(true); raw.emptyPDF();
        const auto revisions = history(r, raw);
        QPDF document; document.setSuppressWarnings(true); document.setAttemptRecovery(false); document.setMaxWarnings(100);
        try { document.processInputSource(std::make_shared<Source>(r)); }
        catch (const QPDFExc& error) {
            if (error.getErrorCode() != qpdf_e_password) { throw; }
            number(r, "encrypted", 1, "pdf.container"); number(r, "password_required", 1, "pdf.container");
            if (security) { security(nullptr, raw, r, revisions); }
            return ParseStatus::Unsupported;
        }
        r.checkpoint();
        value(r, "pdf_version", ValueKind::Text, document.getPDFVersion(), "pdf.container");
        number(r, "encrypted", document.isEncrypted() ? 1 : 0, "pdf.container");
        number(r, "password_required", 0, "pdf.container");
        auto trailer = document.getTrailer(), catalog = document.getRoot(), info = trailer.getKey("/Info");
        if (!info.isNull()) {
            if (!info.isDictionary()) { throw Failure{ParseStatus::Malformed, "PDF Info is not a dictionary", 0}; }
            Reader fields(context, 0, 0, "pdf.info"); scalar_tree(fields, info, "", object_id(info));
            if (info.isIndirect()) {
                const auto id = info.getObjGen(); const auto slot = revisions.slots.find(static_cast<std::uint64_t>(id.getObj()));
                if (slot != revisions.slots.end() && slot->second.type == 1 && slot->second.generation == static_cast<std::uint64_t>(id.getGen())) {
                    static_cast<void>(raw_dictionary(r, raw, slot->second.offset, static_cast<std::uint64_t>(id.getObj()), slot->second.generation));
                }
            }
        }
        Reader catalog_fields(context, 0, 0, "pdf.catalog");
        for (const auto* key : {"/Lang", "/Version", "/MarkInfo", "/ViewerPreferences", "/PageMode", "/PageLayout"}) {
            auto field = catalog.getKey(key); if (!field.isNull()) { scalar_tree(catalog_fields, field, std::string(key).substr(1), object_id(catalog)); }
        }
        auto xmp = catalog.getKey("/Metadata");
        if (!xmp.isNull()) {
            Reader xml(context, 0, 0, "pdf.xmp"); const auto data = stream_bytes(xml, xmp, true);
            value(xml, "xmp_packet", ValueKind::DecodedBytes, std::string(data.begin(), data.end()), object_id(xmp));
            inspect_xmp_data(xml, data, object_id(xmp));
        }
        Attachments attachments(r); auto names = catalog.getKey("/Names");
        if (names.isDictionary()) { auto files = names.getKey("/EmbeddedFiles"); if (!files.isNull()) { attachments.names(files); } }
        attachments.associated(catalog.getKey("/AF"), "Catalog associated files");
        Pages pages(r, attachments); pages.walk(catalog.getKey("/Pages")); pages.finish(); attachments.finish();
        if (security) { security(&document, raw, r, revisions); }
        bool warnings = false;
        for (const auto& warning : document.getWarnings()) {
            r.entry(); warnings = true; value(r, "library_warning", ValueKind::Text, warning.getMessageDetail(), "pdf.container");
        }
        return warnings ? ParseStatus::Malformed : ParseStatus::Complete;
    } catch (const Failure& error) {
        r.diagnostic("failure", error.message, std::min(error.offset, r.size)); return error.status;
    } catch (const QPDFExc& error) {
        const auto at = error.getFilePosition(); r.diagnostic("library_error", error.getMessageDetail(), at > 0 ? std::min<std::uint64_t>(static_cast<std::uint64_t>(at), r.size) : 0);
        return ParseStatus::Malformed;
    }
}
}
namespace metaxtract::native {
namespace {
class PdfMetadataParser final : public Parser {
public:
    const ParserCapability& capability() const noexcept override {
        static const ParserCapability value{"pdf.metadata", "Read-only PDF metadata pages attachments and revisions", "12.4.2", Probe | Extract, {"pdf"}};
        return value;
    }
    ParseStatus run(ParserContext& context) const override { return formats::pdf::inspect(context); }
};
}
std::unique_ptr<Parser> pdf_metadata_parser() { return std::make_unique<PdfMetadataParser>(); }
}
