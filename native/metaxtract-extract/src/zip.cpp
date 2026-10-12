#include "zip.hpp"

#include <array>
#include <set>
#include <zlib.h>

namespace metaxtract::native::formats {
namespace {

std::uint32_t checksum(const std::span<const std::uint8_t> bytes, Reader& reader) {
    auto value = crc32(0, Z_NULL, 0);
    for (std::size_t at = 0; at < bytes.size();) {
        reader.checkpoint();
        const auto count = std::min<std::size_t>(65536, bytes.size() - at);
        value = crc32(value, bytes.data() + at, static_cast<uInt>(count));
        at += count;
    }
    return static_cast<std::uint32_t>(value);
}

void extras(Reader& reader, ZipEntry& entry, bool need_size, bool need_compressed, bool need_offset, bool need_disk) {
    std::uint64_t at = 0;
    bool zip64 = false;
    while (at < reader.size) {
        reader.entry();
        const auto header = reader.read(at, 4);
        const auto code = integer(header, 0, 2, true), length = integer(header, 2, 2, true);
        auto part = reader.window(at + 4, length, reader.group + "." + hex_key(code));
        part.put("extra_data", ValueKind::Bytes, "", 0, part.read(0, length), "", code, true);
        if (code == 1) {
            if (zip64) { throw Failure{ParseStatus::Malformed, "Duplicate ZIP64 size fields", part.base}; }
            zip64 = true;
            std::uint64_t cursor = 0;
            const auto next = [&]() { const auto value = integer(part.read(cursor, 8), 0, 8, true); cursor += 8; return value; };
            if (need_size) { entry.size = next(); }
            if (need_compressed) { entry.compressed_size = next(); }
            if (need_offset) { entry.local_offset = next(); }
            if (need_disk && integer(part.read(cursor, 4), 0, 4, true) != 0) {
                throw Failure{ParseStatus::Unsupported, "Split ZIP packages are not supported", part.base};
            }
        } else if (code == 0x5455 && length >= 1) {
            const auto flags = part.read(0, 1)[0];
            std::uint64_t position = 1;
            if ((flags & 1U) != 0) { part.number("modification_unix_seconds", position, 4, true); position += 4; }
            if ((flags & 2U) != 0 && length - position >= 4) { part.number("access_unix_seconds", position, 4, true); position += 4; }
            if ((flags & 4U) != 0 && length - position >= 4) { part.number("creation_unix_seconds", position, 4, true); }
        } else if (code == 0x000a && length >= 4) {
            std::uint64_t position = 4;
            while (position < length) {
                const auto h = part.read(position, 4);
                const auto tag = integer(h, 0, 2, true), amount = integer(h, 2, 2, true);
                part.require(position + 4, amount);
                if (tag == 1 && amount == 24) {
                    part.number("modification_filetime", position + 4, 8, true);
                    part.number("access_filetime", position + 12, 8, true);
                    part.number("creation_filetime", position + 20, 8, true);
                }
                position += 4 + amount;
            }
        }
        at += 4 + length;
    }
    if ((need_size || need_compressed || need_offset || need_disk) && !zip64) {
        throw Failure{ParseStatus::Malformed, "ZIP64 sentinel has no extended size fields", reader.base};
    }
}

}

bool unsafe_member_name(const std::string_view name) {
    if (name.empty() || name.front() == '/' || name.front() == '\\' || name.find('\0') != std::string_view::npos ||
        name.find('\\') != std::string_view::npos || name.find(':') != std::string_view::npos) { return true; }
    std::size_t at = 0;
    while (at <= name.size()) {
        const auto slash = name.find('/', at);
        const auto part = name.substr(at, slash == std::string_view::npos ? name.size() - at : slash - at);
        if (part == "." || part == "..") { return true; }
        if (slash == std::string_view::npos) { break; }
        at = slash + 1;
    }
    return false;
}

std::vector<ZipEntry> zip_directory(Reader& source) {
    if (source.size < 22) { throw Failure{ParseStatus::Malformed, "ZIP end record is missing", source.base}; }
    const auto tail_size = std::min<std::uint64_t>(source.size, 65557);
    const auto tail = source.read(source.size - tail_size, tail_size);
    std::uint64_t end = source.size;
    for (std::size_t at = tail.size() - 22;; --at) {
        if (starts(tail, std::string_view("PK\5\6", 4), at) &&
            integer(tail, at + 20, 2, true) == tail.size() - at - 22) {
            end = source.size - tail_size + at; break;
        }
        if (at == 0) { break; }
    }
    if (end == source.size) { throw Failure{ParseStatus::Malformed, "ZIP end record does not terminate the archive", source.base}; }
    const auto eocd = source.read(end, 22);
    if (integer(eocd, 4, 2, true) != 0 || integer(eocd, 6, 2, true) != 0 ||
        integer(eocd, 8, 2, true) != integer(eocd, 10, 2, true)) {
        throw Failure{ParseStatus::Unsupported, "Split ZIP packages are not supported", source.base + end};
    }
    std::uint64_t count = integer(eocd, 10, 2, true), length = integer(eocd, 12, 4, true), at = integer(eocd, 16, 4, true);
    std::uint64_t directory_end_limit = end;
    bool zip64 = count == 0xffff || length == 0xffffffff || at == 0xffffffff;
    if (zip64) {
        if (end < 20) { throw Failure{ParseStatus::Malformed, "ZIP64 locator missing", source.base + end}; }
        const auto locator = source.read(end - 20, 20);
        if (!starts(locator, std::string_view("PK\6\7", 4)) || integer(locator, 4, 4, true) != 0 || integer(locator, 16, 4, true) != 1) {
            throw Failure{ParseStatus::Malformed, "Invalid ZIP64 locator", source.base + end - 20};
        }
        const auto offset = integer(locator, 8, 8, true);
        const auto record = source.read(offset, 56);
        if (!starts(record, std::string_view("PK\6\6", 4)) || integer(record, 4, 8, true) < 44 ||
            offset > end - 20 || end - 20 - offset < 12 || integer(record, 4, 8, true) > end - 20 - offset - 12 ||
            integer(record, 16, 4, true) != 0 || integer(record, 20, 4, true) != 0 ||
            integer(record, 24, 8, true) != integer(record, 32, 8, true)) {
            throw Failure{ParseStatus::Malformed, "Invalid ZIP64 end record", source.base + offset};
        }
        count = integer(record, 32, 8, true); length = integer(record, 40, 8, true); at = integer(record, 48, 8, true);
        directory_end_limit = offset;
    }
    if (at > directory_end_limit || length > directory_end_limit - at || count > length / 46) {
        throw Failure{ParseStatus::Malformed, "ZIP central directory escapes its end record", source.base + std::min(at, source.size)};
    }
    source.derived("zip64", zip64 ? 1 : 0);
    source.derived("entry_count", count);
    source.put("archive_comment", ValueKind::Bytes, "", end + 22, source.read(end + 22, integer(eocd, 20, 2, true)));
    const auto directory_start = at;
    const auto directory_end = at + length;
    std::vector<ZipEntry> result;
    std::set<std::string> names;
    for (std::uint64_t index = 0; index < count; ++index) {
        source.entry(); source.checkpoint();
        if (directory_end - at < 46) { throw Failure{ParseStatus::Malformed, "Truncated ZIP central entry", source.base + at}; }
        const auto header = source.read(at, 46);
        if (!starts(header, std::string_view("PK\1\2", 4))) { throw Failure{ParseStatus::Malformed, "Invalid ZIP central entry signature", source.base + at}; }
        const auto name_size = integer(header, 28, 2, true), extra_size = integer(header, 30, 2, true), comment_size = integer(header, 32, 2, true);
        const auto extent = 46 + name_size + extra_size + comment_size;
        if (extent > directory_end - at) { throw Failure{ParseStatus::Malformed, "ZIP central name/extra/comment escapes its directory", source.base + at}; }
        auto field = source.window(at, extent, source.group + ".entry" + std::to_string(index));
        const auto name = field.read(46, name_size);
        ZipEntry entry{.name = {name.begin(), name.end()}, .central_offset = at,
            .local_offset = integer(header, 42, 4, true), .data_offset = 0,
            .compressed_size = integer(header, 20, 4, true), .size = integer(header, 24, 4, true),
            .crc = static_cast<std::uint32_t>(integer(header, 16, 4, true)),
            .flags = static_cast<std::uint16_t>(integer(header, 8, 2, true)),
            .compression = static_cast<std::uint16_t>(integer(header, 10, 2, true)),
            .zip64_sizes = integer(header, 20, 4, true) == 0xffffffff || integer(header, 24, 4, true) == 0xffffffff, .index = index};
        if (!source.context.budget().reserve_allocation(8 * (sizeof(ZipEntry) + name.size()))) {
            throw Failure{ParseStatus::Internal, "ZIP directory allocation budget exhausted", field.base};
        }
        field.put("filename", ValueKind::Bytes, "", 46, name, (entry.flags & 0x800U) ? "UTF-8" : "CP437");
        field.number("version_made_by", 4, 2, true); field.number("version_needed", 6, 2, true);
        field.number("flags", 8, 2, true); field.number("compression_method", 10, 2, true);
        field.number("dos_time", 12, 2, true); field.number("dos_date", 14, 2, true);
        field.number("crc32", 16, 4, true); field.number("external_attributes", 38, 4, true);
        field.put("entry_comment", ValueKind::Bytes, "", 46 + name_size + extra_size, field.read(46 + name_size + extra_size, comment_size));
        field.derived("unsafe_name", unsafe_member_name(entry.name) ? 1 : 0);
        if (!names.insert(entry.name).second) { field.diagnostic("duplicate_name", "Repeated ZIP member name retained by entry index", 46); }
        auto extra = field.window(46 + name_size, extra_size, field.group + ".extra");
        const auto disk = integer(header, 34, 2, true);
        if (disk != 0 && disk != 0xffff) { throw Failure{ParseStatus::Unsupported, "Split ZIP member is not supported", field.base + 34}; }
        extras(extra, entry, entry.size == 0xffffffff, entry.compressed_size == 0xffffffff, entry.local_offset == 0xffffffff, disk == 0xffff);
        field.derived("compressed_size", entry.compressed_size); field.derived("uncompressed_size", entry.size);
        field.derived("local_header_offset", entry.local_offset);
        if (entry.local_offset >= directory_start) { throw Failure{ParseStatus::Malformed, "ZIP member header overlaps its central directory", field.base}; }
        const auto local = source.read(entry.local_offset, 30);
        if (!starts(local, std::string_view("PK\3\4", 4)) || integer(local, 6, 2, true) != entry.flags || integer(local, 8, 2, true) != entry.compression || integer(local, 26, 2, true) != name_size) {
            throw Failure{ParseStatus::Malformed, "ZIP local and central headers disagree", source.base + entry.local_offset};
        }
        const auto local_name = source.read(entry.local_offset + 30, name_size);
        if (local_name != name) { throw Failure{ParseStatus::Malformed, "ZIP local and central names disagree", source.base + entry.local_offset + 30}; }
        const auto local_extra = integer(local, 28, 2, true);
        source.require(entry.local_offset + 30, name_size + local_extra);
        const auto local_size = integer(local, 22, 4, true), local_compressed = integer(local, 18, 4, true);
        if (local_size == 0xffffffff || local_compressed == 0xffffffff) {
            const auto data = source.read(entry.local_offset + 30 + name_size, local_extra);
            bool found = false;
            for (std::size_t position = 0; position < data.size();) {
                if (data.size() - position < 4) { throw Failure{ParseStatus::Malformed, "Truncated ZIP local extra field", source.base + entry.local_offset}; }
                const auto code = integer(data, position, 2, true), amount = integer(data, position + 2, 2, true);
                if (amount > data.size() - position - 4) { throw Failure{ParseStatus::Malformed, "ZIP local extra field escapes header", source.base + entry.local_offset}; }
                if (code == 1) {
                    if (found) { throw Failure{ParseStatus::Malformed, "Duplicate local ZIP64 size fields", source.base + entry.local_offset}; }
                    found = true;
                    std::size_t cursor = position + 4;
                    const auto next = [&]() {
                        if (position + 4 + amount - cursor < 8) { throw Failure{ParseStatus::Malformed, "Missing local ZIP64 size", source.base + entry.local_offset}; }
                        const auto value = integer(data, cursor, 8, true); cursor += 8; return value;
                    };
                    if (local_size == 0xffffffff) {
                        const auto value = next();
                        if ((entry.flags & 8U) == 0 && value != entry.size) { throw Failure{ParseStatus::Malformed, "Local ZIP64 length disagrees with directory", source.base + entry.local_offset}; }
                    }
                    if (local_compressed == 0xffffffff) {
                        const auto value = next();
                        if ((entry.flags & 8U) == 0 && value != entry.compressed_size) { throw Failure{ParseStatus::Malformed, "Local ZIP64 compressed length disagrees with directory", source.base + entry.local_offset}; }
                    }
                }
                position += 4 + static_cast<std::size_t>(amount);
            }
            if (!found) { throw Failure{ParseStatus::Malformed, "Local ZIP64 sentinel has no size fields", source.base + entry.local_offset}; }
        }
        entry.data_offset = entry.local_offset + 30 + name_size + local_extra;
        source.require(entry.data_offset, entry.compressed_size);
        auto local_metadata = source.window(entry.local_offset + 30 + name_size, local_extra, field.group + ".local_extra");
        auto local_entry = entry;
        extras(local_metadata, local_entry, false, false, false, false);
        if (entry.data_offset > directory_start || entry.compressed_size > directory_start - entry.data_offset) {
            throw Failure{ParseStatus::Malformed, "ZIP payload overlaps its central directory", source.base + entry.data_offset};
        }
        if ((entry.flags & 8U) == 0 && (integer(local, 14, 4, true) != entry.crc ||
            (integer(local, 18, 4, true) != 0xffffffff && integer(local, 18, 4, true) != entry.compressed_size) ||
            (integer(local, 22, 4, true) != 0xffffffff && integer(local, 22, 4, true) != entry.size))) {
            throw Failure{ParseStatus::Malformed, "ZIP local CRC/sizes disagree with its directory", source.base + entry.local_offset};
        }
        if ((entry.flags & 8U) != 0) {
            const auto descriptor_at = entry.data_offset + entry.compressed_size;
            const unsigned width = entry.zip64_sizes || local_size == 0xffffffff || local_compressed == 0xffffffff ? 8U : 4U;
            const auto prefix = source.read(descriptor_at, 4);
            bool matched = false;
            for (const auto signature : {4U, 0U}) {
                if (signature == 4U && !starts(prefix, std::string_view("PK\7\10", 4))) { continue; }
                const auto extent_descriptor = signature + 4U + width * 2U;
                if (descriptor_at > directory_start || extent_descriptor > directory_start - descriptor_at) { continue; }
                const auto data = source.read(descriptor_at, extent_descriptor);
                if (integer(data, signature, 4, true) == entry.crc && integer(data, signature + 4U, width, true) == entry.compressed_size && integer(data, signature + 4U + width, width, true) == entry.size) { matched = true; break; }
            }
            if (!matched) {
                throw Failure{ParseStatus::Malformed, "ZIP data descriptor disagrees with directory", source.base + descriptor_at};
            }
        }
        field.derived("data_offset", entry.data_offset);
        result.push_back(std::move(entry)); at += extent;
    }
    if (at != directory_end) {
        source.diagnostic("central_trailing_data", "Extra records after ZIP central entries are not interpreted", at);
    }
    return result;
}

std::vector<std::uint8_t> zip_payload(Reader& source, const ZipEntry& entry) {
    if ((entry.flags & 0x2041U) != 0) { throw Failure{ParseStatus::Unsupported, "Encrypted ZIP metadata is not decoded", source.base + entry.data_offset}; }
    auto compressed = source.read(entry.data_offset, entry.compressed_size);
    std::vector<std::uint8_t> bytes;
    if (entry.compression == 0) {
        if (!source.context.budget().consume_decoded(compressed.size())) { throw Failure{ParseStatus::Internal, "ZIP decoded budget exhausted", source.base}; }
        bytes = std::move(compressed);
    } else if (entry.compression == 8) { bytes = inflate_metadata(source, compressed, -15, entry.size); }
    else { throw Failure{ParseStatus::Unsupported, "ZIP compression method is not decoded", source.base + entry.data_offset}; }
    if (bytes.size() != entry.size || checksum(bytes, source) != entry.crc) {
        throw Failure{ParseStatus::Malformed, "ZIP decoded length or CRC does not match its directory", source.base + entry.data_offset};
    }
    return bytes;
}

}
