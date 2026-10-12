#include "ole.hpp"

namespace metaxtract::native::formats {
namespace {
void word(Reader& reader, const std::string& packet) {
    const auto header = reader.read(0, 32);
    if (integer(header, 0, 2, true) != 0xa5ec) { throw Failure{ParseStatus::Malformed, "WordDocument has an invalid FIB signature", 0}; }
    reader.decoded("legacy_family", ValueKind::Text, "word", packet);
    reader.number("fib_version", 2, 2, true); reader.number("language_id", 6, 2, true);
    reader.number("fib_flags", 10, 2, true); reader.number("encryption_key_or_length", 14, 4, true);
    reader.number("environment", 18, 1); reader.number("text_start", 24, 4, true); reader.number("text_end", 28, 4, true);
    const auto flags = integer(header, 10, 2, true);
    reader.derived("template_flag", flags & 1U); reader.derived("quick_save_count", (flags >> 4U) & 15U);
    reader.derived("encrypted", (flags >> 8U) & 1U); reader.derived("table_stream_selector", (flags >> 9U) & 1U);
    reader.derived("read_only_recommended", (flags >> 10U) & 1U); reader.derived("write_reservation", (flags >> 11U) & 1U);
    reader.derived("obfuscated", (flags >> 15U) & 1U);
    if ((flags & 0x100U) != 0) { reader.derived("password_required", 1); throw Failure{ParseStatus::Unsupported, "Encrypted Word content requires a password; FIB facts retained", 0}; }
    if (reader.size > 32) {
        const auto words = integer(reader.read(32, 2), 0, 2, true);
        reader.put("fib_short_fields", ValueKind::Bytes, "", 34, reader.read(34, words * 2), "binary");
        const auto at = 34 + words * 2;
        const auto longs = integer(reader.read(at, 2), 0, 2, true);
        reader.put("fib_long_fields", ValueKind::Bytes, "", at + 2, reader.read(at + 2, longs * 4), "binary");
        if (longs >= 11) {
            reader.number("document_byte_count", at + 2, 4, true);
            reader.number("main_text_character_count", at + 2 + 3 * 4, 4, true);
            reader.number("footnote_character_count", at + 2 + 4 * 4, 4, true);
            reader.number("header_character_count", at + 2 + 5 * 4, 4, true);
            reader.number("comment_character_count", at + 2 + 7 * 4, 4, true);
        }
    }
}
void workbook(Reader& reader, const std::string& packet) {
    std::uint64_t at = 0, index = 0;
    bool first = true, encrypted = false;
    std::uint32_t page = 0;
    std::uint64_t version = 0;
    while (at < reader.size) {
        reader.entry(); reader.checkpoint();
        const auto header = reader.read(at, 4);
        const auto type = integer(header, 0, 2, true), length = integer(header, 2, 2, true);
        auto record = reader.window(at + 4, length, reader.group + ".biff" + std::to_string(index++));
        record.derived("biff_record_type", type); record.derived("biff_record_length", length);
        record.put("biff_payload", ValueKind::Bytes, "", 0, record.read(0, length), "binary", type, true);
        if (first) {
            if (type != 0x0809 && type != 0x0409 && type != 0x0209 && type != 0x0009) { throw Failure{ParseStatus::Malformed, "Workbook does not start with a BIFF BOF record", 0}; }
            first = false; reader.decoded("legacy_family", ValueKind::Text, "spreadsheet", packet);
            version = integer(record.read(0, 2), 0, 2, true);
            record.number("biff_version", 0, 2, true); record.number("biff_substream_type", 2, 2, true);
        }
        if (type == 0x002f) {
            encrypted = true; record.derived("password_required", 1);
            // Later records can contain encrypted lengths/data: do not interpret them.
            break;
        }
        if (type == 0x0042) {
            page = static_cast<std::uint32_t>(integer(record.read(0, 2), 0, 2, true)); record.derived("workbook_code_page", page);
        } else if (type == 0x0085) {
            record.number("sheet_stream_offset", 0, 4, true); record.number("sheet_visibility", 4, 1); record.number("sheet_type", 5, 1);
            const auto count = record.read(6, 1)[0];
            const bool biff8 = version >= 0x0600;
            const bool wide = biff8 && (record.read(7, 1)[0] & 1U) != 0;
            const auto bytes = record.read(biff8 ? 8 : 7, static_cast<std::uint64_t>(count) * (wide ? 2U : 1U));
            record.decoded("sheet_name_bytes", ValueKind::DecodedBytes, {bytes.begin(), bytes.end()}, packet, 3, "binary");
            const auto text = decode_text(record, bytes, wide ? "utf-16le" : biff8 ? "iso-8859-1" : std::to_string(page));
            if (!text.supported) { throw Failure{ParseStatus::Unsupported, "Legacy BIFF sheet code page is unsupported", 0}; }
            if (!text.valid) { throw Failure{ParseStatus::Malformed, "Invalid BIFF sheet name encoding", 0}; }
            record.decoded("sheet_name", ValueKind::Text, text.value, packet);
        } else if (type == 0x0012 || type == 0x0013 || type == 0x0019) {
            record.number(type == 0x0012 ? "workbook_protect" : type == 0x0013 ? "password_verifier" : "window_protect", 0, 2, true);
        } else if (type == 0x01ae || type == 0x0023 || type == 0x0017) {
            record.derived("external_link_record", 1);
        }
        at += 4 + length;
        if (type == 0x000a) {
            // A workbook stream can contain further worksheet substreams.
            if (at == reader.size || (at < reader.size && reader.read(at, 1)[0] == 0 && reader.size - at < 4)) { break; }
        }
    }
    if (encrypted) { throw Failure{ParseStatus::Unsupported, "Encrypted BIFF content is retained without decryption", 0}; }
}
void powerpoint_records(Reader& reader, const std::string& packet, const std::uint32_t depth) {
    if (!reader.context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "PowerPoint record depth budget exceeded", 0}; }
    std::uint64_t at = 0, index = 0;
    while (at < reader.size) {
        reader.entry(); reader.checkpoint();
        const auto header = reader.read(at, 8);
        const auto options = integer(header, 0, 2, true), type = integer(header, 2, 2, true), length = integer(header, 4, 4, true);
        auto record = reader.window(at + 8, length, reader.group + ".ppt" + std::to_string(index++));
        record.derived("ppt_record_type", type); record.derived("ppt_record_version", options & 15U);
        record.derived("ppt_record_instance", options >> 4U);
        if ((options & 15U) == 15) { powerpoint_records(record, packet, depth + 1); }
        else {
            record.put("ppt_record_payload", ValueKind::Bytes, "", 0, record.read(0, length), "binary", type, true);
            if (type == 4085) {
                record.number("last_slide_id", 0, 4, true); record.number("last_edit_version", 4, 2, true);
                record.number("previous_user_edit_offset", 8, 4, true); record.number("persist_directory_offset", 12, 4, true);
            } else if (type == 1001 && length >= 24) {
                record.number("slide_width_master_units", 0, 4, true); record.number("slide_height_master_units", 4, 4, true);
                record.number("notes_width_master_units", 8, 4, true); record.number("notes_height_master_units", 12, 4, true);
            } else if (type == 4000 || type == 4008 || type == 4026) {
                const auto bytes = record.read(0, length);
                const auto text = decode_text(record, bytes, type == 4008 ? "iso-8859-1" : "utf-16le");
                if (!text.valid) { throw Failure{ParseStatus::Malformed, "Invalid PowerPoint Unicode record", 0}; }
                record.decoded("ppt_text", ValueKind::Text, text.value, packet);
            } else if (type == 12052) {
                record.derived("password_required", 1);
                throw Failure{ParseStatus::Unsupported, "PowerPoint encryption record retained without decryption", 0};
            }
        }
        at += 8 + length;
    }
}
}

void inspect_legacy_stream(Reader& reader, const std::string& name, const std::string& packet, const std::uint32_t depth) {
    if (name == "WordDocument") { word(reader, packet); }
    else if (name == "Workbook" || name == "Book") { workbook(reader, packet); }
    else if (name == "PowerPoint Document") {
        reader.decoded("legacy_family", ValueKind::Text, "presentation", packet); powerpoint_records(reader, packet, depth);
    } else if (name == "Current User") {
        const auto header = reader.read(0, 8);
        if (integer(header, 2, 2, true) != 4086) { throw Failure{ParseStatus::Malformed, "Invalid PowerPoint CurrentUserAtom record", 0}; }
        const auto length = integer(header, 4, 4, true);
        auto atom = reader.window(8, length, reader.group + ".current-user");
        atom.number("current_user_header_token", 4, 4, true); atom.number("current_edit_offset", 8, 4, true);
        atom.number("current_user_document_version", 14, 2, true);
        const auto size = integer(atom.read(12, 2), 0, 2, true);
        const auto bytes = atom.read(20, size);
        atom.put("current_user_name_bytes", ValueKind::Bytes, "", 20, bytes, "ANSI_unspecified");
        if (length >= 24 + size + size * 2) {
            const auto unicode = atom.read(24 + size, size * 2);
            const auto text = decode_text(atom, unicode, "utf-16le");
            if (!text.valid) { throw Failure{ParseStatus::Malformed, "Invalid CurrentUser Unicode name", 0}; }
            atom.decoded("current_user_name", ValueKind::Text, text.value, packet);
        }
    } else if (name == "\1Ole10Native") {
        const auto amount = integer(reader.read(0, 4), 0, 4, true);
        reader.number("native_data_size", 0, 4, true);
        const auto bytes = reader.read(4, amount);
        reader.decoded("embedded_payload", ValueKind::DecodedBytes, {bytes.begin(), bytes.end()}, packet + "/native", 3, "binary");
        if (4 + amount < reader.size) { reader.diagnostic("native_trailing_data", "Bytes after declared OLE native data are retained in the stream payload", 4 + amount); }
        Reader native(reader.context, bytes, reader.group + ".native", packet + "/native");
        if (starts(bytes, std::string_view("\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1", 8))) {
            const auto result = inspect_compound(native, depth + 1);
            if (result != ParseStatus::Complete) { throw Failure{result, "Nested OLE native data is partial", 0}; }
        }
    } else if (name == "\1CompObj") {
        // Preserve the class header and length-prefixed user type / ProgID.
        reader.put("compobj_header", ValueKind::Bytes, "", 0, reader.read(0, 28), "binary");
        std::uint64_t at = 28;
        for (unsigned index = 0; index < 3 && at < reader.size; ++index) {
            const auto length = integer(reader.read(at, 4), 0, 4, true); at += 4;
            if (length == 0xffffffff) { reader.number("clipboard_format", at, 4, true); at += 4; continue; }
            reader.put(index == 0 ? "user_type_bytes" : index == 1 ? "clipboard_name_bytes" : "prog_id_bytes",
                       ValueKind::Bytes, "", at, reader.read(at, length), "ANSI_unspecified");
            at += length;
        }
    }
}
}
