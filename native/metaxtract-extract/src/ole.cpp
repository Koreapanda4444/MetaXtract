#include "ole.hpp"

#include <array>

namespace metaxtract::native::formats {
namespace {
constexpr std::uint32_t free_sector = 0xffffffffU, end_chain = 0xfffffffeU;
constexpr std::uint32_t fat_sector = 0xfffffffdU, difat_sector = 0xfffffffcU;
constexpr std::string_view signature("\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1", 8);

struct Directory final {
    std::string name;
    std::string path;
    std::uint8_t kind = 0;
    std::uint32_t left = free_sector, right = free_sector, child = free_sector, start = end_chain;
    std::uint64_t size = 0;
    bool reachable = false;
};

class Compound final {
    Reader& source;
    std::uint64_t sector_size = 512, sector_count = 0;
    std::uint16_t major = 3;
    std::vector<std::uint32_t> fat, mini_fat;
    std::vector<std::uint8_t> owners, mini_owners, mini_stream;
    std::vector<Directory> directory;
    ParseStatus status = ParseStatus::Complete;
    std::uint32_t nesting;
    std::string packet_prefix;

    void charge(const std::uint64_t amount) const {
        if (!source.context.budget().reserve_allocation(amount)) {
            throw Failure{ParseStatus::Internal, "OLE allocation budget exceeded", source.base};
        }
    }
    std::uint64_t offset(const std::uint32_t id) const {
        if (id >= sector_count) { throw Failure{ParseStatus::Malformed, "OLE sector index escapes the file", source.base}; }
        return (static_cast<std::uint64_t>(id) + 1) * sector_size;
    }
    std::vector<std::uint32_t> chain(const std::uint32_t start, const std::vector<std::uint32_t>& table,
                                    std::vector<std::uint8_t>& usage, const std::uint64_t limit) {
        std::vector<std::uint32_t> result;
        auto id = start;
        while (id != end_chain) {
            source.entry(); source.checkpoint();
            if (id >= usage.size() || id >= table.size() || result.size() >= limit || usage[id] != 0) {
                throw Failure{ParseStatus::Malformed, "OLE chain is cyclic, shared, reserved, or outside its allocation table", source.base};
            }
            usage[id] = 1;
            charge(16);
            result.push_back(id);
            id = table[id];
        }
        return result;
    }
    std::vector<std::uint8_t> regular(const std::vector<std::uint32_t>& sectors, const std::uint64_t length,
                                      const std::string& group, const bool emit) {
        if (length > sectors.size() * sector_size) {
            throw Failure{ParseStatus::Malformed, "OLE stream is longer than its sector chain", source.base};
        }
        charge(length);
        if (!source.context.budget().consume_decoded(length)) { throw Failure{ParseStatus::Internal, "OLE decoded budget exceeded", source.base}; }
        std::vector<std::uint8_t> result;
        result.reserve(static_cast<std::size_t>(length));
        for (const auto id : sectors) {
            source.checkpoint();
            const auto count = std::min<std::uint64_t>(sector_size, length - result.size());
            if (count == 0) { break; }
            auto fragment = source.window(offset(id), count, group);
            auto bytes = fragment.read(0, count);
            if (emit) { fragment.put("stream_fragment", ValueKind::Bytes, "", 0, bytes, "binary"); }
            result.insert(result.end(), bytes.begin(), bytes.end());
        }
        return result;
    }
    void read_header() {
        const auto header = source.read(0, 512);
        if (!starts(header, signature)) { throw Failure{ParseStatus::Unsupported, "Not an OLE compound file", source.base}; }
        major = static_cast<std::uint16_t>(integer(header, 26, 2, true));
        const auto shift = integer(header, 30, 2, true);
        if (integer(header, 28, 2, true) != 0xfffe || !((major == 3 && shift == 9) || (major == 4 && shift == 12)) ||
            integer(header, 32, 2, true) != 6 || integer(header, 56, 4, true) != 4096) {
            throw Failure{ParseStatus::Malformed, "Invalid CFB version, byte order, sector sizes, or mini-stream cutoff", source.base};
        }
        sector_size = std::uint64_t{1} << shift;
        if (source.size < sector_size || source.size % sector_size != 0) {
            throw Failure{ParseStatus::Malformed, "CFB file does not contain whole sectors", source.base};
        }
        sector_count = source.size / sector_size - 1;
        if (sector_count > 0xfffffffaU) { throw Failure{ParseStatus::Unsupported, "CFB sector count is not representable", source.base}; }
        charge(4 * (sector_count + 1));
        owners.assign(static_cast<std::size_t>(sector_count), 0);
        source.number("minor_version", 24, 2, true); source.number("major_version", 26, 2, true);
        source.number("transaction_signature", 52, 4, true);
        source.derived("sector_size", sector_size); source.derived("sector_count", sector_count);
        const auto fat_count = integer(header, 44, 4, true), difat_count = integer(header, 72, 4, true);
        if (fat_count > sector_count || difat_count > sector_count) { throw Failure{ParseStatus::Malformed, "CFB table counts exceed file sectors", source.base}; }
        charge(8 * fat_count);
        std::vector<std::uint32_t> fat_ids;
        const auto add_fat = [&](const std::uint32_t id) {
            if (id == free_sector) { return; }
            static_cast<void>(offset(id));
            if (owners[id] != 0 || fat_ids.size() >= fat_count) { throw Failure{ParseStatus::Malformed, "Duplicate or excessive CFB FAT sector", source.base}; }
            owners[id] = 2; fat_ids.push_back(id);
        };
        for (std::size_t at = 76; at < 512; at += 4) { add_fat(static_cast<std::uint32_t>(integer(header, at, 4, true))); }
        auto difat = static_cast<std::uint32_t>(integer(header, 68, 4, true));
        std::vector<std::uint32_t> difat_ids;
        for (std::uint64_t index = 0; index < difat_count; ++index) {
            source.entry(); source.checkpoint();
            static_cast<void>(offset(difat));
            if (owners[difat] != 0) { throw Failure{ParseStatus::Malformed, "Cyclic or shared CFB DIFAT sector", source.base}; }
            owners[difat] = 3; charge(16); difat_ids.push_back(difat);
            const auto bytes = source.read(offset(difat), sector_size);
            for (std::size_t at = 0; at < bytes.size() - 4; at += 4) { add_fat(static_cast<std::uint32_t>(integer(bytes, at, 4, true))); }
            difat = static_cast<std::uint32_t>(integer(bytes, bytes.size() - 4, 4, true));
        }
        if ((difat_count != 0 && difat != end_chain) || fat_ids.size() != fat_count ||
            fat_count * (sector_size / 4) < sector_count) {
            throw Failure{ParseStatus::Malformed, "Incomplete CFB DIFAT/FAT coverage", source.base};
        }
        charge(4 * fat_count * (sector_size / 4));
        fat.reserve(static_cast<std::size_t>(fat_count * (sector_size / 4)));
        for (const auto id : fat_ids) {
            source.entry();
            const auto bytes = source.read(offset(id), sector_size);
            for (std::size_t at = 0; at < bytes.size(); at += 4) { fat.push_back(static_cast<std::uint32_t>(integer(bytes, at, 4, true))); }
        }
        for (const auto id : fat_ids) {
            if (fat[id] != fat_sector) { throw Failure{ParseStatus::Malformed, "CFB FAT sector is not marked FATSECT", source.base + offset(id)}; }
        }
        for (const auto id : difat_ids) {
            if (fat[id] != difat_sector) { throw Failure{ParseStatus::Malformed, "CFB DIFAT sector is not marked DIFSECT", source.base + offset(id)}; }
        }
        const auto directory_ids = chain(static_cast<std::uint32_t>(integer(header, 48, 4, true)), fat, owners, sector_count);
        if (directory_ids.empty() || (major == 3 && integer(header, 40, 4, true) != 0) ||
            (major == 4 && integer(header, 40, 4, true) != directory_ids.size())) {
            throw Failure{ParseStatus::Malformed, "CFB directory sector count disagrees with version", source.base};
        }
        for (const auto id : directory_ids) {
            for (std::uint64_t at = 0; at < sector_size; at += 128) {
                source.entry(); charge(512);
                const auto index = directory.size();
                auto item = source.window(offset(id) + at, 128, source.group + ".directory" + std::to_string(index));
                const auto bytes = item.read(0, 128);
                Directory entry;
                entry.kind = bytes[66];
                entry.left = static_cast<std::uint32_t>(integer(bytes, 68, 4, true));
                entry.right = static_cast<std::uint32_t>(integer(bytes, 72, 4, true));
                entry.child = static_cast<std::uint32_t>(integer(bytes, 76, 4, true));
                entry.start = static_cast<std::uint32_t>(integer(bytes, 116, 4, true));
                entry.size = integer(bytes, 120, 8, true);
                if (entry.kind != 0) {
                    const auto length = integer(bytes, 64, 2, true);
                    item.put("directory_record", ValueKind::Bytes, "", 0, bytes, "binary");
                    if (length < 2 || length > 64 || length % 2 != 0 || integer(bytes, static_cast<std::size_t>(length - 2), 2, true) != 0 ||
                        (entry.kind != 1 && entry.kind != 2 && entry.kind != 5) || (major == 3 && (entry.size >> 32U) != 0)) {
                        throw Failure{ParseStatus::Malformed, "Invalid CFB directory type, Unicode name, or stream size", item.base};
                    }
                    const auto name = decode_text(item, std::span(bytes).first(static_cast<std::size_t>(length - 2)), "utf-16le");
                    if (!name.valid) { throw Failure{ParseStatus::Malformed, "CFB directory name has invalid UTF-16", item.base}; }
                    entry.name = name.value;
                    item.put("name", ValueKind::Text, entry.name, 0, {bytes.begin(), bytes.begin() + static_cast<std::ptrdiff_t>(length)}, "UTF-16LE");
                    item.number("object_type", 66, 1); item.number("color", 67, 1);
                    item.number("left_sibling", 68, 4, true); item.number("right_sibling", 72, 4, true); item.number("child", 76, 4, true);
                    item.put("class_id", ValueKind::Bytes, "", 80, item.read(80, 16), "GUID");
                    item.number("state_bits", 96, 4, true); item.number("creation_filetime", 100, 8, true);
                    item.number("modification_filetime", 108, 8, true); item.number("start_sector", 116, 4, true); item.number("stream_size", 120, 8, true);
                }
                directory.push_back(std::move(entry));
            }
        }
        if (directory.front().kind != 5) { throw Failure{ParseStatus::Malformed, "CFB root directory entry is missing", source.base}; }
        directory.front().reachable = true; directory.front().path = "/";
        struct Pending { std::uint32_t id; std::string parent; std::uint32_t depth; };
        std::vector<Pending> pending;
        charge(128); pending.push_back({directory.front().child, "/", nesting});
        while (!pending.empty()) {
            source.checkpoint();
            auto next = std::move(pending.back()); pending.pop_back();
            if (next.id == free_sector) { continue; }
            if (!source.context.budget().check_depth(next.depth)) { throw Failure{ParseStatus::Internal, "CFB storage depth budget exceeded", source.base}; }
            if (next.id >= directory.size() || directory[next.id].kind == 0 || directory[next.id].kind == 5 || directory[next.id].reachable) {
                throw Failure{ParseStatus::Malformed, "CFB directory links are cyclic, shared, or invalid", source.base};
            }
            auto& entry = directory[next.id]; entry.reachable = true;
            charge(4 * (next.parent.size() + entry.name.size()) + 512);
            entry.path = next.parent + path_component(entry.name);
            pending.push_back({entry.left, next.parent, next.depth}); pending.push_back({entry.right, next.parent, next.depth});
            if (entry.kind == 1) { pending.push_back({entry.child, entry.path + "/", next.depth + 1}); }
            else if (entry.child != free_sector) { throw Failure{ParseStatus::Malformed, "CFB stream has directory children", source.base}; }
        }
        const auto mini_count = integer(header, 64, 4, true);
        if (mini_count > sector_count) { throw Failure{ParseStatus::Malformed, "CFB mini-FAT count exceeds file", source.base}; }
        if (mini_count != 0) {
            const auto ids = chain(static_cast<std::uint32_t>(integer(header, 60, 4, true)), fat, owners, sector_count);
            if (ids.size() != mini_count) { throw Failure{ParseStatus::Malformed, "CFB mini-FAT chain count disagrees with header", source.base}; }
            charge(4 * mini_count * (sector_size / 4));
            for (const auto id : ids) {
                const auto bytes = source.read(offset(id), sector_size);
                for (std::size_t at = 0; at < bytes.size(); at += 4) { mini_fat.push_back(static_cast<std::uint32_t>(integer(bytes, at, 4, true))); }
            }
        }
        const auto& root = directory.front();
        if (root.size != 0) {
            const auto ids = chain(root.start, fat, owners, sector_count);
            mini_stream = regular(ids, root.size, source.group + ".mini-stream", true);
        }
        charge(mini_stream.size() / 64 + 1);
        mini_owners.assign(mini_stream.size() / 64, 0);
    }
    std::vector<std::uint8_t> stream(const Directory& entry, const std::string& group) {
        if (entry.size == 0) { return {}; }
        if (entry.size >= 4096) {
            const auto ids = chain(entry.start, fat, owners, sector_count);
            return regular(ids, entry.size, group, true);
        }
        const auto ids = chain(entry.start, mini_fat, mini_owners, mini_owners.size());
        if (entry.size > ids.size() * 64ULL) { throw Failure{ParseStatus::Malformed, "CFB mini-stream chain is shorter than its stream", source.base}; }
        charge(entry.size);
        if (!source.context.budget().consume_decoded(entry.size)) { throw Failure{ParseStatus::Internal, "OLE decoded budget exceeded", source.base}; }
        std::vector<std::uint8_t> result; result.reserve(static_cast<std::size_t>(entry.size));
        for (const auto id : ids) {
            source.checkpoint();
            const auto at = static_cast<std::size_t>(id) * 64;
            const auto count = static_cast<std::size_t>(std::min<std::uint64_t>(64, entry.size - result.size()));
            if (count == 0) { break; }
            if (at > mini_stream.size() || count > mini_stream.size() - at) { throw Failure{ParseStatus::Malformed, "CFB mini sector escapes root stream", source.base}; }
            result.insert(result.end(), mini_stream.begin() + static_cast<std::ptrdiff_t>(at), mini_stream.begin() + static_cast<std::ptrdiff_t>(at + count));
        }
        return result;
    }
public:
    Compound(Reader& reader, const std::uint32_t depth) : source(reader), nesting(depth), packet_prefix(reader.group) {}
    ParseStatus run() {
        read_header();
        std::uint64_t macros = 0, embeddings = 0, orphans = 0, streams = 0;
        for (std::size_t index = 1; index < directory.size(); ++index) {
            const auto& entry = directory[index];
            if (entry.kind == 0) { continue; }
            source.entry(); source.checkpoint();
            const auto packet = packet_prefix + "/entry" + std::to_string(index) + "/" + path_component(entry.name);
            Reader field(source.context, std::span<const std::uint8_t>{}, source.group + ".entry" + std::to_string(index), packet);
            field.decoded("storage_path", ValueKind::Text, entry.reachable ? entry.path : "/orphan/" + std::to_string(index), packet);
            field.derived("orphan", entry.reachable ? 0 : 1); orphans += entry.reachable ? 0 : 1;
            macros += entry.name == "VBA" || entry.name == "Macros" || entry.name == "_VBA_PROJECT" ? 1 : 0;
            if (entry.kind != 2) { continue; }
            ++streams;
            try {
                const auto bytes = stream(entry, field.group);
                Reader payload(source.context, bytes, field.group, packet);
                payload.decoded("stream_payload", ValueKind::DecodedBytes, {bytes.begin(), bytes.end()}, packet, 3, "binary");
                if (entry.name == "\5SummaryInformation" || entry.name == "\5DocumentSummaryInformation") {
                    inspect_properties(payload, packet);
                }
                inspect_legacy_stream(payload, entry.name, packet, nesting);
                if (entry.name == "EncryptedPackage" || entry.name == "EncryptionInfo") {
                    payload.derived("password_required", 1);
                    if (status == ParseStatus::Complete) { status = ParseStatus::Unsupported; }
                }
                if (entry.name == "\1Ole10Native") { ++embeddings; }
                if (starts(bytes, signature)) {
                    ++embeddings;
                    const auto nested = inspect_compound(payload, nesting + 1);
                    if (nested == ParseStatus::Malformed || status == ParseStatus::Complete) { status = nested; }
                }
            } catch (const Failure& error) {
                field.diagnostic("stream_failure", error.message, 0);
                if (source.context.budget().status() != BudgetStatus::Ok) { throw; }
                if (error.status == ParseStatus::Malformed || status == ParseStatus::Complete) { status = error.status; }
            }
        }
        source.derived("stream_count", streams); source.derived("macro_marker_count", macros);
        source.derived("embedded_object_count", embeddings); source.derived("orphan_count", orphans);
        return status;
    }
};
}

ParseStatus inspect_compound(Reader& reader, const std::uint32_t depth) {
    if (!reader.context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "Nested OLE depth budget exceeded", reader.base}; }
    return Compound(reader, depth).run();
}
}

namespace metaxtract::native {
namespace {
class OleParser final : public Parser {
    const ParserCapability& capability() const noexcept override {
        static const ParserCapability value{"document.ole", "OLE compound and legacy Office metadata", "1", Extract | EnumerateChildren, {"doc", "dot", "xls", "xlt", "ppt", "pps", "pot", "ole", "cfb"}};
        return value;
    }
    ParseStatus run(ParserContext& context) const override {
        formats::Reader reader(context, 0, context.source_size(), "document.ole");
        try {
            const auto header = reader.read(0, std::min<std::uint64_t>(reader.size, 8));
            if (!formats::starts(header, std::string_view("\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1", 8))) { return ParseStatus::Unsupported; }
            return formats::inspect_compound(reader);
        } catch (const formats::Failure& error) {
            reader.diagnostic("failure", error.message, error.offset >= reader.base ? error.offset - reader.base : 0);
            return error.status;
        }
    }
};
}
std::unique_ptr<Parser> document_ole_parser() { return std::make_unique<OleParser>(); }
}
