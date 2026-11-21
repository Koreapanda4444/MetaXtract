#include "format.hpp"
#include <memory>

namespace metaxtract::native {
namespace {
class ExportedMetadata final : public Parser {
public:
    const ParserCapability& capability() const noexcept override {
        static const ParserCapability value{"metadata.export", "MetaXtract exported metadata packet capture", "builtin", Extract, {"json"}};
        return value;
    }
    ParseStatus run(ParserContext& context) const override {
        using namespace formats;
        Reader r(context, 0, context.source_size(), "metadata.export");
        try {
            if (!context.budget().check_member_size(r.size)) { throw Failure{ParseStatus::Internal, "Exported metadata size budget", 0}; }
            if (r.size > 2U * 1024U * 1024U) { return ParseStatus::Unsupported; }
            const auto data = r.read(0, r.size);
            std::size_t first = starts(data, std::string_view("\xef\xbb\xbf", 3)) ? 3 : 0;
            while (first < data.size() && (data[first] == ' ' || data[first] == '\t' || data[first] == '\r' || data[first] == '\n')) { ++first; }
            if (first == data.size() || data[first] != '{') { return ParseStatus::Unsupported; }
            r.put("json_packet", ValueKind::Bytes, "", 0, data);
            r.decoded("validation_scope", ValueKind::Text, "raw_packet_only; schema and relationships evaluated by analysis", "exported-metadata");
            return ParseStatus::Complete;
        } catch (const Failure& failure) { return failure.status; }
    }
};
}
std::unique_ptr<Parser> exported_metadata_parser() { return std::make_unique<ExportedMetadata>(); }
}
