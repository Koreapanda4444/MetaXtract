#include "format.hpp"
#include "xml.hpp"
#include <memory>

namespace metaxtract::native {
namespace {
class XmpSidecar final : public Parser {
public:
    const ParserCapability& capability() const noexcept override {
        static const ParserCapability value{"xmp.sidecar", "MetaXtract XMP sidecar parser", "builtin", Probe | Extract, {"xmp"}};
        return value;
    }
    ParseStatus run(ParserContext& context) const override {
        using namespace formats;
        Reader r(context, 0, context.source_size(), "xmp.sidecar");
        try {
            if (!context.budget().check_member_size(r.size)) { throw Failure{ParseStatus::Internal, "XMP sidecar size limit", 0}; }
            auto bytes = r.read(0, r.size);
            {
                XmlBudgetScope scope(context.budget()); pugi::xml_document document;
                if (!document.load_buffer(bytes.data(), bytes.size(), pugi::parse_default | pugi::parse_doctype, pugi::encoding_auto)) {
                    throw Failure{ParseStatus::Malformed, "Invalid XMP sidecar XML", 0};
                }
                const auto root = document.document_element();
                const auto name = expanded_xml_name(root, root.name());
                if (name != "{adobe:ns:meta/}xmpmeta" && name != "{adobe:ns:meta/}xapmeta" &&
                    name != "{http://www.w3.org/1999/02/22-rdf-syntax-ns#}RDF") { return ParseStatus::Unsupported; }
            }
            r.put("packet", ValueKind::Bytes, "", 0, bytes);
            inspect_xmp_data(r, bytes, "xmp-sidecar");
            return ParseStatus::Complete;
        } catch (const Failure& failure) {
            if (context.budget().status() == BudgetStatus::Ok) { r.diagnostic("invalid_sidecar", failure.message, failure.offset); }
            return failure.status;
        }
    }
};
}
std::unique_ptr<Parser> xmp_sidecar_parser() { return std::make_unique<XmpSidecar>(); }
}
