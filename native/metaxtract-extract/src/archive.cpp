#include "format.hpp"
#include "archive_budget.hpp"
#include "text.hpp"
#include "zip.hpp"

#include <archive.h>
#include <archive_entry.h>
#include <memory>

namespace metaxtract::native::formats {
namespace {
struct Input final {
    Reader& reader;
    std::uint64_t position = 0;
    std::vector<std::uint8_t> block;
    bool failed = false;
};
la_ssize_t read_block(archive* handle, void* client, const void** data) noexcept {
    auto& input = *static_cast<Input*>(client);
    try {
        const auto size = std::min<std::uint64_t>(65536, input.reader.size - input.position);
        if (size == 0) { *data = nullptr; return 0; }
        input.block = input.reader.read(input.position, size);
        input.position += size; *data = input.block.data();
        return static_cast<la_ssize_t>(size);
    } catch (...) {
        input.failed = true; archive_set_error(handle, 0, "Bounded archive input read failed"); return -1;
    }
}
la_int64_t seek_block(archive*, void* client, la_int64_t offset, int whence) noexcept {
    auto& input = *static_cast<Input*>(client);
    const auto base = whence == SEEK_SET ? 0 : whence == SEEK_CUR ? input.position : whence == SEEK_END ? input.reader.size : UINT64_MAX;
    if (base == UINT64_MAX) { return -1; }
    const auto magnitude = offset < 0 ? static_cast<std::uint64_t>(-(offset + 1)) + 1 : static_cast<std::uint64_t>(offset);
    if ((offset < 0 && magnitude > base) || (offset >= 0 && magnitude > input.reader.size - base)) { return -1; }
    input.position = offset < 0 ? base - magnitude : base + magnitude;
    return static_cast<la_int64_t>(input.position);
}
la_int64_t skip_block(archive*, void* client, la_int64_t count) noexcept {
    auto& input = *static_cast<Input*>(client);
    if (count <= 0) { return 0; }
    const auto amount = std::min<std::uint64_t>(static_cast<std::uint64_t>(count), input.reader.size - input.position);
    input.position += amount;
    return static_cast<la_int64_t>(amount);
}
void bytes(Reader& reader, const std::string& key, const char* value, const std::string& packet) {
    if (value) { reader.decoded(key, ValueKind::DecodedBytes, value, packet, 2, "library_bytes"); }
}
void signed_number(Reader& reader, const std::string& key, const std::int64_t value, const std::string& packet) {
    reader.decoded(key, ValueKind::Signed, std::to_string(value), packet, 2, "integer");
}

ParseStatus inspect_archive(Reader& source) {
    ArchiveBudgetScope budget(source.context.budget());
    std::unique_ptr<archive, decltype(&archive_read_free)> handle(archive_read_new(), archive_read_free);
    if (!handle) { throw Failure{ParseStatus::AllocationFailure, "Archive reader allocation failed", 0}; }
    auto* library = handle.get();
    // Register native implementations individually: the 'all' convenience API
    // can register external-program fallbacks for unavailable codecs.
    archive_read_support_filter_none(library);
    archive_read_support_filter_gzip(library);
    archive_read_support_filter_bzip2(library);
    archive_read_support_filter_xz(library);
    archive_read_support_filter_lzma(library);
    archive_read_support_filter_compress(library);
    archive_read_support_format_zip(library);
    archive_read_support_format_tar(library);
    archive_read_support_format_7zip(library);
    archive_read_support_format_raw(library);
    Input input{source, 0, {}, false};
    archive_read_set_callback_data(library, &input);
    archive_read_set_read_callback(library, read_block);
    archive_read_set_seek_callback(library, seek_block);
    archive_read_set_skip_callback(library, skip_block);
    if (archive_read_open1(library) != ARCHIVE_OK) {
        throw Failure{input.failed ? ParseStatus::ReadFailure : ParseStatus::Malformed, "Archive header/filter could not be opened", 0};
    }
    auto status = ParseStatus::Complete;
    std::uint64_t index = 0;
    archive_entry* entry = nullptr;
    while (true) {
        source.entry(); source.checkpoint();
        const auto result = archive_read_next_header(library, &entry);
        if (result == ARCHIVE_EOF) { break; }
        if (result < ARCHIVE_WARN || !entry) {
            throw Failure{ParseStatus::Malformed, "Archive member header is malformed or unsupported", 0};
        }
        if (index == 0) {
            bytes(source, "archive_format", archive_format_name(library), "archive");
            for (int filter = 0; filter < archive_filter_count(library); ++filter) {
                bytes(source, "compression_filter", archive_filter_name(library, filter), "archive");
            }
            if (archive_format(library) == ARCHIVE_FORMAT_RAW && archive_filter_code(library, 0) == ARCHIVE_FILTER_NONE) {
                return ParseStatus::Unsupported;
            }
        }
        const auto* raw_name = archive_entry_pathname(entry);
        const std::string name = raw_name ? raw_name : "";
        if (!source.context.budget().reserve_allocation(8 * name.size() + 512)) {
            throw Failure{ParseStatus::Internal, "Archive member index budget exceeded", 0};
        }
        const auto packet = "archive.entry" + std::to_string(index) + "/" + path_component(name);
        Reader field(source.context, std::span<const std::uint8_t>{}, "container.archive.entry" + std::to_string(index), packet);
        field.derived("member_index", index++);
        field.decoded("filename", ValueKind::DecodedBytes, name, packet, 2, "library_bytes");
        if (const auto* unicode = archive_entry_pathname_utf8(entry)) {
            const std::string value(unicode);
            const auto decoded = decode_text(field, std::span(reinterpret_cast<const std::uint8_t*>(value.data()), value.size()), "utf-8");
            if (decoded.valid) { field.decoded("filename_utf8", ValueKind::Text, value, packet, 2); }
        }
        field.derived("unsafe_name", unsafe_member_name(name) ? 1 : 0);
        field.derived("file_type", archive_entry_filetype(entry)); field.derived("mode", archive_entry_mode(entry));
        signed_number(field, "uid", archive_entry_uid(entry), packet); signed_number(field, "gid", archive_entry_gid(entry), packet);
        bytes(field, "user_name", archive_entry_uname(entry), packet); bytes(field, "group_name", archive_entry_gname(entry), packet);
        bytes(field, "symlink_target", archive_entry_symlink(entry), packet); bytes(field, "hardlink_target", archive_entry_hardlink(entry), packet);
        if (archive_entry_mtime_is_set(entry)) {
            signed_number(field, "mtime_seconds", archive_entry_mtime(entry), packet); signed_number(field, "mtime_nanoseconds", archive_entry_mtime_nsec(entry), packet);
        }
        if (archive_entry_atime_is_set(entry)) { signed_number(field, "atime_seconds", archive_entry_atime(entry), packet); }
        if (archive_entry_ctime_is_set(entry)) { signed_number(field, "ctime_seconds", archive_entry_ctime(entry), packet); }
        if (archive_entry_birthtime_is_set(entry)) { signed_number(field, "birthtime_seconds", archive_entry_birthtime(entry), packet); }
        const auto declared = archive_entry_size(entry);
        if (archive_entry_size_is_set(entry)) { signed_number(field, "declared_size", declared, packet); }
        const bool encrypted = archive_entry_is_encrypted(entry) != 0;
        field.derived("encrypted", encrypted ? 1 : 0);
        if (encrypted) {
            field.derived("password_required", 1); if (status == ParseStatus::Complete) { status = ParseStatus::Unsupported; }
            if (archive_read_data_skip(library) < ARCHIVE_WARN) { break; }
            continue;
        }
        if (archive_entry_filetype(entry) != AE_IFREG || archive_entry_hardlink(entry)) {
            if (archive_read_data_skip(library) < ARCHIVE_WARN) { throw Failure{ParseStatus::Malformed, "Archive special member cannot be skipped", 0}; }
            continue;
        }
        std::vector<std::uint8_t> payload;
        while (true) {
            field.checkpoint();
            const void* block = nullptr; std::size_t count = 0; la_int64_t at = 0;
            const auto code = archive_read_data_block(library, &block, &count, &at);
            if (code == ARCHIVE_EOF) { break; }
            if (code != ARCHIVE_OK || at < 0 || (count != 0 && !block) || static_cast<std::uint64_t>(at) > UINT64_MAX - count) {
                field.diagnostic("data_failure", "Archive member decoding/checksum failed; prior bytes retained", 0);
                status = ParseStatus::Malformed; break;
            }
            const auto end = static_cast<std::uint64_t>(at) + count;
            if (end > payload.size()) {
                const auto increase = end - payload.size();
                if (increase > UINT64_MAX / 2) { throw Failure{ParseStatus::Malformed, "Archive sparse extent overflows allocation size", 0}; }
                if (!source.context.budget().consume_decoded(increase) || !source.context.budget().reserve_allocation(2 * increase)) {
                    throw Failure{ParseStatus::Internal, "Archive expanded member exceeds budget", 0};
                }
                payload.resize(static_cast<std::size_t>(end), 0);
            }
            if (count != 0) { std::memcpy(payload.data() + static_cast<std::size_t>(at), block, count); }
        }
        if (archive_entry_size_is_set(entry) && declared >= 0 && payload.size() != static_cast<std::uint64_t>(declared)) {
            field.diagnostic("size_mismatch", "Decoded archive size differs from its declaration", 0); status = ParseStatus::Malformed;
        }
        field.derived("decoded_size", payload.size());
        field.decoded("member_payload", ValueKind::DecodedBytes, {payload.begin(), payload.end()}, packet, 3, "binary");
        if (result == ARCHIVE_WARN) { field.diagnostic("header_warning", "Archive library reported a noncanonical header; claims retained", 0); }
    }
    source.derived("member_count", index);
    return status;
}
}
}

namespace metaxtract::native {
namespace {
class ArchiveParser final : public Parser {
    const ParserCapability& capability() const noexcept override {
        static const ParserCapability value{"container.archive", "Native archive and compressed stream metadata", "1", Extract | EnumerateChildren,
            {"zip", "tar", "7z", "gz", "bz2", "xz", "lzma", "Z", "tgz", "tbz2", "txz"}};
        return value;
    }
    ParseStatus run(ParserContext& context) const override {
        formats::Reader reader(context, 0, context.source_size(), "container.archive");
        try { return formats::inspect_archive(reader); }
        catch (const formats::Failure& error) { reader.diagnostic("failure", error.message, 0); return error.status; }
    }
};
}
std::unique_ptr<Parser> archive_metadata_parser() { return std::make_unique<ArchiveParser>(); }
}
