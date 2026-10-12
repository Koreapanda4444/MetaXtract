#include "format.hpp"
#include "text.hpp"

namespace metaxtract::native::formats {
namespace {
std::uint64_t num(Reader& r, std::uint64_t at, unsigned width, bool little = true) {
    return integer(r.read(at, width), 0, width, little);
}
void raw(Reader& r, const std::string& key, std::uint64_t at, std::uint64_t size) {
    r.put(key, ValueKind::Bytes, "", at, r.read(at, size));
}
void string_field(Reader& r, const std::string& key, std::uint64_t at, std::uint64_t size, const std::string& encoding = "UTF-8") {
    auto bytes = r.read(at, size); const auto decoded = decode_text(r, bytes, encoding);
    r.put(key, decoded.valid && decoded.supported ? ValueKind::Utf8Text : ValueKind::Bytes, decoded.value, at, std::move(bytes), encoding);
}
void cstring(Reader& r, const std::string& key, std::uint64_t at, std::uint64_t maximum) {
    r.require(at, 0); std::uint64_t length = 0;
    while (length < std::min(maximum, r.size - at)) {
        r.checkpoint(); const auto block = r.read(at + length, std::min<std::uint64_t>(256, std::min(maximum, r.size - at) - length));
        const auto end = std::find(block.begin(), block.end(), 0);
        if (end != block.end()) { string_field(r, key, at, length + static_cast<std::uint64_t>(end - block.begin())); return; }
        length += block.size();
    }
    throw Failure{ParseStatus::Malformed, "Binary string is not terminated within its range", r.base + at};
}
std::uint64_t align4(std::uint64_t value) { return (value + 3U) & ~std::uint64_t{3}; }
void version_blocks(Reader& r, std::uint32_t depth) {
    if (!r.context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "PE version depth limit", r.base}; }
    std::uint64_t at = 0;
    while (at < r.size) {
        r.entry(); r.require(at, 6); const auto length = num(r, at, 2), count = num(r, at + 2, 2), type = num(r, at + 4, 2);
        if (length < 6) { throw Failure{ParseStatus::Malformed, "Invalid PE version block length", r.base + at}; }
        auto block = r.window(at, length, r.group + ".block" + std::to_string(at)); std::uint64_t end = 6;
        while (end + 2 <= block.size && num(block, end, 2) != 0) { end += 2; }
        block.require(end, 2); string_field(block, "name", 6, end - 6, "UTF-16LE");
        const auto value_at = align4(end + 2), value_size = count * (type == 1 ? 2 : 1);
        if (value_size || value_at <= block.size) { block.require(value_at, value_size); }
        if (value_size) { string_field(block, "value", value_at, value_size, type == 1 ? "UTF-16LE" : "binary"); }
        const auto children = align4(value_at + value_size);
        if (children < block.size) { auto child = block.window(children, block.size - children, block.group); version_blocks(child, depth + 1); }
        at += align4(length);
    }
}
void pe(Reader& r) {
    r.require(0, 64); const auto header = num(r, 60, 4); r.require(header, 24);
    if (!starts(r.read(header, 4), std::string_view("PE\0\0", 4))) { throw Failure{ParseStatus::Malformed, "Invalid PE signature", header}; }
    r.number("machine", header + 4, 2, true); r.number("timestamp_raw", header + 8, 4, true);
    r.number("characteristics", header + 22, 2, true); r.number("symbol_count", header + 16, 4, true);
    const auto count = num(r, header + 6, 2), optional_size = num(r, header + 20, 2), optional_at = header + 24;
    auto optional = r.window(optional_at, optional_size, "binary.pe.optional"); const auto magic = num(optional, 0, 2); const bool wide = magic == 0x20b;
    if (!wide && magic != 0x10b) { throw Failure{ParseStatus::Unsupported, "Unsupported PE optional header", optional_at}; }
    optional.number("magic", 0, 2, true); optional.number("entry_point_rva", 16, 4, true); optional.number("image_base", wide ? 24 : 28, wide ? 8 : 4, true);
    optional.number("section_alignment", 32, 4, true); optional.number("file_alignment", 36, 4, true); optional.number("image_size", 56, 4, true);
    optional.number("header_size", 60, 4, true); optional.number("checksum", 64, 4, true); optional.number("subsystem", 68, 2, true); optional.number("dll_characteristics", 70, 2, true);
    const auto directory_at = wide ? 112U : 96U, directory_count_at = wide ? 108U : 92U; const auto directories = num(optional, directory_count_at, 4);
    optional.require(directory_at, directories * 8);
    struct Section { std::uint64_t address, offset, size; }; std::vector<Section> sections;
    if (!r.context.budget().reserve_allocation(count * sizeof(Section))) { throw Failure{ParseStatus::Internal, "PE section allocation limit", header}; }
    for (std::uint64_t i = 0; i < count; ++i) {
        r.entry(); auto field = r.window(optional_at + optional_size + i * 40, 40, "binary.pe.section" + std::to_string(i));
        raw(field, "name_raw", 0, 8); field.number("virtual_size", 8, 4, true); field.number("rva", 12, 4, true);
        field.number("raw_size", 16, 4, true); field.number("raw_offset", 20, 4, true); field.number("characteristics", 36, 4, true);
        const auto offset = num(field, 20, 4), size = num(field, 16, 4); r.require(offset, size); sections.push_back({num(field, 12, 4), offset, size});
    }
    auto mapped = [&](std::uint64_t address, std::uint64_t size) {
        if (address < num(optional, 60, 4)) { r.require(address, size); return address; }
        for (const auto& section : sections) {
            if (address >= section.address && address - section.address <= section.size && size <= section.size - (address - section.address)) { return section.offset + address - section.address; }
        }
        throw Failure{ParseStatus::Malformed, "PE RVA has no file-backed range", header};
    };
    for (std::uint64_t i = 0; i < directories; ++i) {
        r.entry(); auto field = optional.window(directory_at + i * 8, 8, "binary.pe.directory" + std::to_string(i));
        field.number("address", 0, 4, true); field.number("size", 4, 4, true); const auto address = num(field, 0, 4), length = num(field, 4, 4);
        if (!address || !length) { continue; } const auto at = i == 4 ? address : mapped(address, length); r.require(at, length);
        if (i == 4) {
            auto certificate = r.window(at, length, "binary.pe.certificates"); raw(certificate, "certificate_data", 0, length);
            certificate.diagnostic("verification_unavailable", "Certificate bytes retained without a signature trust assertion", 0);
        } else if (i == 1) {
            for (std::uint64_t position = 0; position + 20 <= length; position += 20) {
                r.entry(); auto entry = r.window(at + position, 20, "binary.pe.import" + std::to_string(position / 20));
                const auto name = num(entry, 12, 4); if (!name) { break; } cstring(r, "import_library", mapped(name, 1), 4096);
                entry.number("timestamp_raw", 4, 4, true); entry.number("lookup_table_rva", 0, 4, true);
            }
        } else if (i == 0) {
            auto exports = r.window(at, length, "binary.pe.exports"); exports.require(0, 40);
            exports.number("timestamp_raw", 4, 4, true); exports.number("ordinal_base", 16, 4, true); exports.number("function_count", 20, 4, true); exports.number("name_count", 24, 4, true);
            const auto name = num(exports, 12, 4); if (name) { cstring(r, "export_library", mapped(name, 1), 4096); }
        } else if (i == 6) {
            if (length % 28) { throw Failure{ParseStatus::Malformed, "Invalid PE debug directory length", at}; }
            for (std::uint64_t position = 0; position < length; position += 28) {
                r.entry(); auto debug = r.window(at + position, 28, "binary.pe.debug" + std::to_string(position / 28)); debug.number("type", 12, 4, true); debug.number("timestamp_raw", 4, 4, true);
                const auto data_at = num(debug, 24, 4), data_size = num(debug, 16, 4); if (!data_size) { continue; } auto data = r.window(data_at, data_size, debug.group); raw(data, "data", 0, data_size);
                if (data_size >= 24 && starts(data.read(0, 4), "RSDS")) { raw(data, "pdb_guid", 4, 16); data.number("pdb_age", 20, 4, true); cstring(data, "pdb_path", 24, data_size - 24); }
            }
        } else if (i == 2) {
            auto resources = r.window(at, length, "binary.pe.resources");
            auto walk = [&](auto&& self, std::uint64_t position, std::uint32_t depth, std::uint64_t type) -> void {
                if (!r.context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "PE resource depth limit", at}; }
                resources.require(position, 16); const auto entries = num(resources, position + 12, 2) + num(resources, position + 14, 2); resources.require(position + 16, entries * 8);
                for (std::uint64_t j = 0; j < entries; ++j) {
                    r.entry(); const auto name = num(resources, position + 16 + j * 8, 4), target = num(resources, position + 20 + j * 8, 4), resource_type = depth == 0 ? name : type;
                    auto field = resources.window(position + 16 + j * 8, 8, "binary.pe.resource" + std::to_string(position) + "_" + std::to_string(j)); field.number("name_or_id", 0, 4, true); field.derived("type_id", resource_type);
                    if (name & 0x80000000U) { const auto n = name & 0x7fffffffU; string_field(resources, "resource_name", n + 2, num(resources, n, 2) * 2, "UTF-16LE"); }
                    if (target & 0x80000000U) { self(self, target & 0x7fffffffU, depth + 1, resource_type); }
                    else {
                        resources.require(target, 16); const auto size = num(resources, target + 4, 4), payload_at = mapped(num(resources, target, 4), size);
                        auto payload = r.window(payload_at, size, field.group); raw(payload, "resource_payload", 0, size); if (resource_type == 16 && size) { version_blocks(payload, depth + 1); }
                    }
                }
            }; walk(walk, 0, 0, 0);
        }
    }
}
void elf(Reader& r) {
    const auto ident = r.read(0, 16); const bool wide = ident[4] == 2, little = ident[5] == 1;
    if ((!wide && ident[4] != 1) || (!little && ident[5] != 2) || ident[6] != 1) { throw Failure{ParseStatus::Unsupported, "Unsupported ELF identification", 0}; }
    r.require(0, wide ? 64 : 52); raw(r, "identification", 0, 16); r.number("type", 16, 2, little); r.number("machine", 18, 2, little); r.number("entry_point", 24, wide ? 8 : 4, little);
    const auto shoff = num(r, wide ? 40 : 32, wide ? 8 : 4, little), stride = num(r, wide ? 58 : 46, 2, little);
    auto count = num(r, wide ? 60 : 48, 2, little), strings_index = num(r, wide ? 62 : 50, 2, little); r.number("flags", wide ? 48 : 36, 4, little);
    if (shoff && stride < (wide ? 64U : 40U)) { throw Failure{ParseStatus::Malformed, "ELF section entry size is too small", shoff}; }
    if (shoff && !count) { count = num(r, shoff + (wide ? 32 : 20), wide ? 8 : 4, little); }
    if (strings_index == 0xffff && shoff) { strings_index = num(r, shoff + (wide ? 40 : 24), 4, little); }
    if (count && (!stride || count > r.size / stride)) { throw Failure{ParseStatus::Malformed, "Invalid ELF section table size", shoff}; } if (count) { r.require(shoff, count * stride); }
    auto section = [&](std::uint64_t i) { return r.window(shoff + i * stride, stride, "binary.elf.section" + std::to_string(i)); };
    auto contents = [&](std::uint64_t i) {
        if (i >= count) { throw Failure{ParseStatus::Malformed, "Invalid ELF linked section index", shoff}; }
        auto entry = section(i); return r.window(num(entry, wide ? 24 : 16, wide ? 8 : 4, little), num(entry, wide ? 32 : 20, wide ? 8 : 4, little), entry.group);
    };
    for (std::uint64_t i = 0; i < count; ++i) {
        r.entry(); auto field = section(i); const auto type = num(field, 4, 4, little); field.number("type", 4, 4, little); field.number("flags", 8, wide ? 8 : 4, little); field.number("address", wide ? 16 : 12, wide ? 8 : 4, little);
        field.number("offset", wide ? 24 : 16, wide ? 8 : 4, little); field.number("size", wide ? 32 : 20, wide ? 8 : 4, little);
        if (strings_index && strings_index < count) { auto names = contents(strings_index); cstring(names, "section_name", num(field, 0, 4, little), names.size); }
        if (type == 8 || type == 0) { continue; } auto data = contents(i);
        if (type == 7) {
            std::uint64_t at = 0;
            while (at < data.size) {
                r.entry(); data.require(at, 12); const auto namesize = num(data, at, 4, little), descsize = num(data, at + 4, 4, little);
                const auto descriptor = at + 12 + align4(namesize), end = descriptor + align4(descsize); data.require(at, end - at); auto note = data.window(at, end - at, field.group + ".note" + std::to_string(at));
                note.number("type", 8, 4, little); string_field(note, "owner", 12, namesize); raw(note, "descriptor", descriptor - at, descsize); at = end;
            }
        } else if (type == 6) {
            const auto linked = num(field, wide ? 40 : 24, 4, little); const auto width = wide ? 8U : 4U;
            if (data.size % (width * 2)) { throw Failure{ParseStatus::Malformed, "ELF dynamic table has a partial entry", data.base}; }
            for (std::uint64_t at = 0; at < data.size; at += width * 2) {
                r.entry(); const auto tag = num(data, at, width, little); if (!tag) { break; } auto dynamic = data.window(at, width * 2, field.group + ".dynamic" + std::to_string(at / (width * 2)));
                dynamic.number("tag", 0, width, little); dynamic.number("value", width, width, little);
                if (tag == 1 || tag == 14 || tag == 15 || tag == 29) { auto strings = contents(linked); cstring(strings, tag == 1 ? "needed_library" : tag == 14 ? "soname" : tag == 15 ? "rpath" : "runpath", num(data, at + width, width, little), strings.size); }
            }
        }
    }
    const auto phoff = num(r, wide ? 32 : 28, wide ? 8 : 4, little), phstride = num(r, wide ? 54 : 42, 2, little); auto phcount = num(r, wide ? 56 : 44, 2, little);
    if (phcount == 0xffff && count) { phcount = num(r, shoff + (wide ? 44 : 28), 4, little); }
    if (phcount && (phstride < (wide ? 56U : 32U) || phcount > r.size / phstride)) { throw Failure{ParseStatus::Malformed, "Invalid ELF program table size", phoff}; } r.require(phoff, phcount * phstride);
    for (std::uint64_t i = 0; i < phcount; ++i) {
        r.entry(); auto field = r.window(phoff + i * phstride, phstride, "binary.elf.program" + std::to_string(i)); field.number("type", 0, 4, little); field.number("flags", wide ? 4 : 24, 4, little);
        field.number("file_offset", wide ? 8 : 4, wide ? 8 : 4, little); field.number("file_size", wide ? 32 : 16, wide ? 8 : 4, little); r.require(num(field, wide ? 8 : 4, wide ? 8 : 4, little), num(field, wide ? 32 : 16, wide ? 8 : 4, little));
    }
}
void macho(Reader& r, std::uint32_t depth) {
    if (!r.context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "Mach-O depth limit", r.base}; } const auto magic = num(r, 0, 4, false);
    if (magic == 0xcafebabe || magic == 0xcafebabf || magic == 0xbebafeca || magic == 0xbfbafeca) {
        const bool little = magic == 0xbebafeca || magic == 0xbfbafeca, wide = magic == 0xcafebabf || magic == 0xbfbafeca;
        const auto count = num(r, 4, 4, little); const auto stride = wide ? 32U : 20U; r.require(8, count * stride);
        for (std::uint64_t i = 0; i < count; ++i) {
            r.entry(); auto field = r.window(8 + i * stride, stride, r.group + ".architecture" + std::to_string(i)); field.number("cpu_type", 0, 4, little); field.number("cpu_subtype", 4, 4, little);
            const auto at = num(field, 8, wide ? 8 : 4, little), size = num(field, wide ? 16 : 12, wide ? 8 : 4, little); auto child = r.window(at, size, field.group); macho(child, depth + 1);
        } return;
    }
    const bool little = magic == 0xcefaedfe || magic == 0xcffaedfe, wide = magic == 0xfeedfacf || magic == 0xcffaedfe;
    if (magic != 0xfeedface && magic != 0xfeedfacf && !little) { throw Failure{ParseStatus::Malformed, "Invalid Mach-O slice magic", r.base}; }
    r.require(0, wide ? 32 : 28); r.number("cpu_type", 4, 4, little); r.number("cpu_subtype", 8, 4, little); r.number("file_type", 12, 4, little); r.number("flags", 24, 4, little);
    const auto count = num(r, 16, 4, little), command_size = num(r, 20, 4, little); auto commands = r.window(wide ? 32 : 28, command_size, r.group); std::uint64_t at = 0;
    for (std::uint64_t i = 0; i < count; ++i) {
        r.entry(); commands.require(at, 8); const auto code = num(commands, at, 4, little), size = num(commands, at + 4, 4, little);
        if (size < 8 || size % 4) { throw Failure{ParseStatus::Malformed, "Invalid Mach-O load command size", commands.base + at}; }
        auto field = commands.window(at, size, r.group + ".command" + std::to_string(i)); field.number("command", 0, 4, little); raw(field, "raw_command", 0, size); const auto kind = code & 0x7fffffffU;
        if (kind == 0x1b) { raw(field, "uuid", 8, 16); }
        else if (kind == 0xc || kind == 0xd || kind == 0x18 || kind == 0x1f || kind == 0x23 || kind == 0x1c || kind == 0x27 || kind == 0xe) {
            const auto offset = num(field, 8, 4, little); if (offset < 12) { throw Failure{ParseStatus::Malformed, "Mach-O string overlaps its header", field.base}; } cstring(field, "path", offset, size);
            if (kind == 0xc || kind == 0xd || kind == 0x18 || kind == 0x1f || kind == 0x23) { field.number("timestamp_raw", 12, 4, little); field.number("current_version_raw", 16, 4, little); field.number("compatibility_version_raw", 20, 4, little); }
        } else if (kind == 0x32) { field.number("platform", 8, 4, little); field.number("minimum_os_raw", 12, 4, little); field.number("sdk_raw", 16, 4, little); }
        else if (kind == 0x1d) { raw(r, "code_signature_data", num(field, 8, 4, little), num(field, 12, 4, little)); r.diagnostic("verification_unavailable", "Signing bytes retained without a trust assertion", 0); }
        at += size;
    }
    if (at != commands.size) { throw Failure{ParseStatus::Malformed, "Mach-O commands do not fill their declared range", commands.base + at}; }
}
ParseStatus script(Reader& r) {
    const auto data = r.read(0, std::min<std::uint64_t>(r.size, 65536)); if (std::find(data.begin(), data.end(), 0) != data.end()) { raw(r, "header_raw", 0, data.size()); return ParseStatus::Unsupported; }
    const auto text = decode_text(r, data, "UTF-8"); if (!text.valid) { raw(r, "header_raw", 0, data.size()); return ParseStatus::Unsupported; }
    string_field(r, "header_text", 0, data.size()); const std::size_t start = starts(data, "\xef\xbb\xbf") ? 3 : 0;
    if (starts(data, "#!", start)) { const auto end = std::find(data.begin() + static_cast<std::ptrdiff_t>(start), data.end(), '\n'); string_field(r, "shebang", start, static_cast<std::uint64_t>(end - data.begin()) - start); }
    else { r.diagnostic("text_language_unconfirmed", "Text header retained; filename does not establish its language", 0); }
    r.derived("header_truncated", data.size() < r.size ? 1 : 0); return ParseStatus::Complete;
}
}
}
namespace metaxtract::native {
namespace {
class BinaryParser final : public Parser {
    const ParserCapability& capability() const noexcept override {
        static const ParserCapability value{"binaries.metadata", "Executable and script structural metadata", "1", Extract, {"pe", "elf", "macho", "script"}}; return value;
    }
    ParseStatus run(ParserContext& context) const override {
        formats::Reader r(context, 0, context.source_size(), "binary");
        try {
            const auto magic = r.read(0, std::min<std::uint64_t>(4, r.size));
            if (formats::starts(magic, "MZ")) { r.group = "binary.pe"; formats::pe(r); }
            else if (formats::starts(magic, "\x7f" "ELF")) { r.group = "binary.elf"; formats::elf(r); }
            else if (magic.size() == 4) {
                const auto code = formats::integer(magic, 0, 4);
                const bool mach = code == 0xcafebabe || code == 0xcafebabf || code == 0xbebafeca || code == 0xbfbafeca || code == 0xfeedface || code == 0xfeedfacf || code == 0xcefaedfe || code == 0xcffaedfe;
                if (!mach) { r.group = "binary.script"; return formats::script(r); }
                if (code == 0xcafebabe) {
                    // JVM class files share this signature. Confirm a file-backed
                    // Mach-O slice before treating the header as a fat binary.
                    bool slice = false;
                    if (r.size >= 28) {
                        const auto header = r.read(0, 28); const auto count = formats::integer(header, 4, 4), at = formats::integer(header, 16, 4), length = formats::integer(header, 20, 4);
                        if (count && count <= (r.size - 8) / 20 && at <= r.size && length >= 4 && length <= r.size - at) {
                            const auto thin = formats::integer(r.read(at, 4), 0, 4); slice = thin == 0xfeedface || thin == 0xfeedfacf || thin == 0xcefaedfe || thin == 0xcffaedfe;
                        }
                    }
                    if (!slice) { formats::raw(r, "ambiguous_header_raw", 0, std::min<std::uint64_t>(32, r.size)); r.diagnostic("ambiguous_signature", "CAFEBABE may identify a JVM class or a fat Mach-O; a Mach-O slice was not confirmed", 0); return ParseStatus::Unsupported; }
                }
                r.group = "binary.macho"; formats::macho(r, 0);
            }
            else { r.group = "binary.script"; return formats::script(r); }
            return ParseStatus::Complete;
        } catch (const formats::Failure& error) { r.diagnostic("failure", error.message, error.offset <= r.size ? error.offset : 0); return error.status; }
    }
};
}
std::unique_ptr<Parser> binary_metadata_parser() { return std::make_unique<BinaryParser>(); }
}
