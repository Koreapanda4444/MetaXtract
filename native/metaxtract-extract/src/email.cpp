#include "mime.hpp"

#include <set>

namespace metaxtract::native::formats {
namespace {
bool whitespace(const char c) { return c == ' ' || c == '\t'; }
std::string_view trim_email(std::string_view text) {
    while (!text.empty() && whitespace(text.front())) { text.remove_prefix(1); }
    while (!text.empty() && whitespace(text.back())) { text.remove_suffix(1); }
    return text;
}
struct Line final { std::size_t start, end, next; bool crlf; };
Line line_at(const std::string_view text, const std::size_t at) {
    const auto newline = text.find('\n', at);
    const auto next = newline == std::string_view::npos ? text.size() : newline + 1;
    auto end = newline == std::string_view::npos ? text.size() : newline;
    const bool crlf = end > at && text[end - 1] == '\r';
    if (crlf) { --end; }
    return {at, end, next, crlf};
}
void raw(Reader& reader, const std::string& key, const std::uint64_t at, const std::span<const std::uint8_t> bytes,
         const std::string& encoding = "binary") {
    if (!reader.context.budget().reserve_allocation(bytes.size())) { throw Failure{ParseStatus::Internal, "Email raw observation allocation exceeded", at}; }
    reader.put(key, ValueKind::Bytes, "", at, {bytes.begin(), bytes.end()}, encoding);
}
struct Header final {
    std::string name, value;
    std::size_t start = 0, end = 0;
};
class Email final {
    ParseStatus status = ParseStatus::Complete;
    std::uint64_t parts = 0, messages = 0, attachments = 0, routes = 0;
    void failure(Reader& reader, const Failure& error) {
        reader.diagnostic("partial", error.message, error.offset);
        if (reader.context.budget().status() != BudgetStatus::Ok) { throw error; }
        if (status == ParseStatus::Complete || error.status == ParseStatus::Malformed) { status = error.status; }
    }
    std::vector<Header> headers(Reader& reader, const std::span<const std::uint8_t> bytes, std::size_t& body) {
        const std::string_view text(reinterpret_cast<const char*>(bytes.data()), bytes.size());
        std::vector<Header> output;
        body = text.size();
        bool noncanonical = false;
        for (std::size_t at = 0; at < text.size();) {
            reader.entry(); reader.checkpoint();
            const auto line = line_at(text, at);
            if (line.next > line.end && !line.crlf && line.next < text.size()) { noncanonical = true; }
            const auto value = text.substr(at, line.end - at);
            if (value.empty()) { body = line.next; break; }
            if (std::any_of(value.begin(), value.end(), [](const auto c) { return static_cast<unsigned char>(c) < 0x20 && c != '\t'; })) {
                raw(reader, "invalid_header_bytes", at, bytes.subspan(at, line.next - at));
                failure(reader, {ParseStatus::Malformed, "Control byte in message header", at}); at = line.next; continue;
            }
            if (whitespace(value.front())) {
                if (output.empty()) {
                    raw(reader, "invalid_header_bytes", at, bytes.subspan(at, line.next - at));
                    failure(reader, {ParseStatus::Malformed, "Header continuation has no preceding field", at});
                } else {
                    if (!reader.context.budget().reserve_allocation(4 * value.size() + 64)) { throw Failure{ParseStatus::Internal, "Header unfolding allocation exceeded", at}; }
                    output.back().value.append(value); output.back().end = line.next;
                }
            } else {
                const auto colon = value.find(':');
                const auto name = value.substr(0, colon);
                if (colon == std::string_view::npos || name.empty() || std::any_of(name.begin(), name.end(), [](const auto c) {
                    return static_cast<unsigned char>(c) <= 0x20 || static_cast<unsigned char>(c) >= 0x7f || c == ':';
                })) {
                    raw(reader, "invalid_header_bytes", at, bytes.subspan(at, line.next - at));
                    if (output.empty()) { throw Failure{ParseStatus::Unsupported, "No recognizable email header block", at}; }
                    failure(reader, {ParseStatus::Malformed, "Malformed message header line", at});
                } else {
                    if (!reader.context.budget().reserve_allocation(8 * value.size() + 256)) { throw Failure{ParseStatus::Internal, "Header allocation exceeded", at}; }
                    output.push_back({ascii_lower(name), std::string(trim_email(value.substr(colon + 1))), at, line.next});
                }
            }
            at = line.next;
        }
        if (noncanonical) { reader.diagnostic("line_endings", "LF-only header lines were accepted and retained", 0); }
        return output;
    }
    void addresses(Reader& field, const std::string_view text, const std::string& packet) {
        // Only explicit angle-addresses are projected; complete address syntax is
        // retained in header text and bytes without guessing group/comment grammar.
        bool quoted = false, escaped = false;
        std::uint32_t comments = 0;
        for (std::size_t at = 0; at < text.size(); ++at) {
            field.checkpoint();
            const auto c = text[at];
            if (escaped) { escaped = false; continue; }
            if (c == '\\' && (quoted || comments != 0)) { escaped = true; continue; }
            if (c == '"' && comments == 0) { quoted = !quoted; continue; }
            if (!quoted && c == '(') { ++comments; continue; }
            if (!quoted && c == ')' && comments != 0) { --comments; continue; }
            if (!quoted && comments == 0 && c == '=' && at + 1 < text.size() && text[at + 1] == '?') {
                const auto word_end = text.find("?=", at + 2);
                if (word_end != std::string_view::npos) { at = word_end + 1; continue; }
            }
            if (quoted || comments != 0 || c != '<') { continue; }
            field.entry();
            const auto end = text.find('>', at + 1);
            if (end == std::string_view::npos) { break; }
            const auto value = text.substr(at + 1, end - at - 1);
            const auto address = decode_text(field, std::span(reinterpret_cast<const std::uint8_t*>(value.data()), value.size()), "utf-8");
            if (address.valid) { field.decoded("angle_address_claim", ValueKind::Text, address.value, packet); }
            at = end;
        }
    }
    std::string first_header(const std::vector<Header>& values, const std::string_view key) const {
        const auto found = std::find_if(values.begin(), values.end(), [key](const auto& h) { return h.name == key; });
        return found == values.end() ? "" : found->value;
    }
    void project(Reader& reader, const std::span<const std::uint8_t> bytes, const std::vector<Header>& values, const std::string& packet) {
        std::set<std::string> seen;
        for (std::size_t index = 0; index < values.size(); ++index) {
            reader.entry(); reader.checkpoint();
            const auto& header = values[index];
            auto field = reader.window(header.start, header.end - header.start, reader.group + ".header" + std::to_string(index));
            raw(field, "header_raw", 0, bytes.subspan(header.start, header.end - header.start));
            field.decoded("header_name", ValueKind::Text, header.name, packet);
            field.decoded("unfolded_bytes", ValueKind::DecodedBytes, header.value, packet, 3, "binary");
            if (!reader.context.budget().reserve_allocation(128 + header.name.size())) { throw Failure{ParseStatus::Internal, "Header duplicate index budget exceeded", 0}; }
            const bool duplicate = !seen.insert(header.name).second;
            field.derived("duplicate_header", duplicate ? 1 : 0);
            if (duplicate && (header.name == "content-type" || header.name == "content-transfer-encoding" || header.name == "content-disposition")) {
                failure(field, {ParseStatus::Malformed, "Duplicate MIME control headers are ambiguous; first retained value controls parsing", 0});
            }
            try {
                const bool encoded = header.name == "subject" || header.name == "from" || header.name == "to" || header.name == "cc" ||
                    header.name == "bcc" || header.name == "reply-to" || header.name == "sender" || header.name == "content-description" || header.name.starts_with("x-");
                const auto text = encoded ? decode_header_words(field, header.value, packet) :
                    decode_header_words(field, header.value.find("=?") == std::string::npos ? header.value : "", packet);
                if (!encoded && header.value.find("=?") != std::string::npos) {
                    field.diagnostic("encoded_word_context", "Encoded word in a structured header is retained without interpretation", 0);
                } else {
                    field.decoded("header_text", ValueKind::Text, text, packet);
                    field.decoded("header." + header.name, ValueKind::Text, text, packet);
                    if (header.name == "from" || header.name == "to" || header.name == "cc" || header.name == "bcc" || header.name == "reply-to" || header.name == "sender") {
                        addresses(field, header.value, packet);
                    }
                }
            } catch (const Failure& error) { failure(field, error); }
            if (header.name == "received") {
                field.derived("route_index_in_header_order", routes++);
                const auto semicolon = header.value.rfind(';');
                if (semicolon != std::string::npos) {
                    field.decoded("received_date_claim_bytes", ValueKind::DecodedBytes, std::string(trim_email(std::string_view(header.value).substr(semicolon + 1))), packet, 3, "binary");
                }
                field.decoded("route_verification", ValueKind::Text, "not_performed", packet, 0);
            }
            if (header.name == "dkim-signature" || header.name == "authentication-results" || header.name.starts_with("arc-") || header.name == "received-spf") {
                field.decoded("authentication_verification", ValueKind::Text, "not_performed", packet, 0);
            }
        }
    }
    void multipart(Reader& reader, const std::span<const std::uint8_t> bytes, const MimeType& type,
                   const std::string& packet, const std::uint32_t depth) {
        const auto boundary = parameter_value(reader, type, "boundary", packet);
        if (boundary.empty() || boundary.size() > 70 || boundary.back() == ' ' ||
            std::any_of(boundary.begin(), boundary.end(), [](const auto c) { return static_cast<unsigned char>(c) < 0x20 || static_cast<unsigned char>(c) >= 0x7f; })) {
            throw Failure{ParseStatus::Malformed, "Invalid or missing multipart boundary", 0};
        }
        reader.decoded("boundary", ValueKind::Text, boundary, packet);
        const std::string_view text(reinterpret_cast<const char*>(bytes.data()), bytes.size());
        const auto marker = "--" + boundary;
        std::size_t start = 0;
        bool opened = false, closed = false;
        std::uint64_t index = 0;
        for (std::size_t at = 0; at < text.size();) {
            reader.checkpoint();
            const auto line = line_at(text, at);
            const auto value = text.substr(at, line.end - at);
            bool delimiter = value.starts_with(marker), closing = false;
            if (delimiter) {
                auto suffix = value.substr(marker.size());
                closing = suffix.starts_with("--");
                if (closing) { suffix.remove_prefix(2); }
                delimiter = std::all_of(suffix.begin(), suffix.end(), [](const auto c) { return whitespace(c); });
            }
            if (delimiter) {
                reader.entry();
                auto end = at;
                // CRLF before a boundary belongs to the delimiter, not the part.
                if (end > start && text[end - 1] == '\n') { --end; if (end > start && text[end - 1] == '\r') { --end; } }
                if (!opened) {
                    raw(reader, "multipart_preamble", 0, bytes.first(end));
                } else if (end >= start) {
                    auto child = reader.window(start, end - start, reader.group + ".part" + std::to_string(index));
                    const auto child_packet = packet + "/part" + std::to_string(index++);
                    try {
                        part(child, bytes.subspan(start, end - start), child_packet, depth + 1, false,
                             type.token == "multipart/digest" ? "message/rfc822" : "text/plain");
                    } catch (const Failure& error) { failure(child, error); }
                }
                if (closing) {
                    closed = true;
                    raw(reader, "multipart_epilogue", line.next, bytes.subspan(line.next));
                    break;
                }
                opened = true; start = line.next;
            }
            at = line.next;
        }
        if (opened && !closed && start < bytes.size()) {
            auto child = reader.window(start, bytes.size() - start, reader.group + ".part" + std::to_string(index));
            try {
                part(child, bytes.subspan(start), packet + "/part" + std::to_string(index++), depth + 1, false,
                     type.token == "multipart/digest" ? "message/rfc822" : "text/plain");
            } catch (const Failure& error) { failure(child, error); }
        }
        reader.derived("multipart_child_count", index);
        if (!opened || !closed) { throw Failure{ParseStatus::Malformed, "Multipart opening or closing boundary is missing; parsed children retained", 0}; }
    }
    void part(Reader& reader, const std::span<const std::uint8_t> bytes, const std::string& packet,
              const std::uint32_t depth, const bool message, const std::string& default_type) {
        reader.entry(); reader.checkpoint();
        if (!reader.context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "MIME nesting depth budget exceeded", 0}; }
        ++parts;
        if (message) { ++messages; }
        std::size_t body = 0;
        const auto values = headers(reader, bytes, body);
        if (message && values.empty()) { throw Failure{ParseStatus::Unsupported, "No email headers were found", 0}; }
        project(reader, bytes, values, packet);
        reader.derived("header_count", values.size());
        auto content = reader.window(body, bytes.size() - body, reader.group + ".body");
        const auto body_bytes = bytes.subspan(body);
        raw(content, "body_encoded", 0, body_bytes);
        const auto declared_type = first_header(values, "content-type");
        const auto type = parse_mime_type(reader, declared_type.empty() ? default_type : declared_type, packet);
        if (type.token.find('/') == std::string::npos || type.token.front() == '/' || type.token.back() == '/' ||
            type.token.find('/', type.token.find('/') + 1) != std::string::npos) { throw Failure{ParseStatus::Malformed, "Content-Type is not a media type", 0}; }
        reader.decoded("media_type", ValueKind::Text, type.token, packet);
        const auto disposition_value = first_header(values, "content-disposition");
        MimeType disposition;
        if (!disposition_value.empty()) {
            try { disposition = parse_mime_type(reader, disposition_value, packet); }
            catch (const Failure& error) { failure(reader, error); }
        }
        if (!disposition.token.empty()) { reader.decoded("disposition", ValueKind::Text, disposition.token, packet); }
        std::string filename;
        try {
            filename = parameter_value(reader, disposition, "filename", packet);
            if (filename.empty()) { filename = parameter_value(reader, type, "name", packet); }
            if (!filename.empty()) {
                reader.decoded("filename", ValueKind::Text, filename, packet);
                reader.derived("unsafe_filename", filename.find('/') != std::string::npos || filename.find('\\') != std::string::npos ||
                    filename.find(':') != std::string::npos || filename.find('\0') != std::string::npos || filename == "." || filename == ".." ? 1 : 0);
            }
        } catch (const Failure& error) { failure(reader, error); }
        const bool attachment = disposition.token == "attachment" || !filename.empty();
        reader.derived("attachment", attachment ? 1 : 0);
        if (attachment) { ++attachments; }
        auto encoding = ascii_lower(trim_email(first_header(values, "content-transfer-encoding")));
        if (encoding.empty()) { encoding = "7bit"; }
        reader.decoded("transfer_encoding", ValueKind::Text, encoding, packet);
        const auto body_packet = packet + "/body";
        const std::string_view body_text(reinterpret_cast<const char*>(body_bytes.data()), body_bytes.size());
        if (type.token.starts_with("multipart/")) {
            if (encoding != "7bit" && encoding != "8bit" && encoding != "binary") {
                throw Failure{ParseStatus::Malformed, "Multipart cannot use an encoded content-transfer representation", 0};
            }
            if (type.token == "multipart/signed" || type.token == "multipart/encrypted") {
                reader.decoded("mime_security_verification", ValueKind::Text, "not_performed", packet, 0);
            }
            multipart(content, body_bytes, type, body_packet, depth);
            return;
        }
        std::vector<std::uint8_t> decoded;
        const bool transformed = encoding == "base64" || encoding == "quoted-printable";
        if (encoding == "base64") { decoded = decode_base64(content, body_text); }
        else if (encoding == "quoted-printable") { decoded = decode_quoted_printable(content, body_text); }
        else if (encoding != "7bit" && encoding != "8bit" && encoding != "binary") {
            throw Failure{ParseStatus::Unsupported, "Unsupported content-transfer encoding; encoded body retained", 0};
        } else if (encoding == "7bit" && std::any_of(body_bytes.begin(), body_bytes.end(), [](const auto c) { return c > 0x7f; })) {
            failure(content, {ParseStatus::Malformed, "7bit body contains non-ASCII bytes; original bytes retained", 0});
        }
        const auto payload = transformed ? std::span<const std::uint8_t>(decoded) : body_bytes;
        Reader extracted(reader.context, payload, content.group + ".decoded", body_packet);
        extracted.decoded("body_payload", ValueKind::DecodedBytes, {payload.begin(), payload.end()}, body_packet, 3, "binary");
        if (attachment) {
            extracted.derived("attachment_size", payload.size());
        }
        if (type.token == "message/rfc822" || type.token == "message/global") {
            // Identity transfer keeps physical ranges; decoded transfer uses a
            // separate packet identity and never invents a source file offset.
            auto nested = transformed ? Reader(reader.context, payload, reader.group + ".message", body_packet) :
                content.window(0, payload.size(), reader.group + ".message");
            try { part(nested, payload, body_packet + "/message", depth + 1, true, "text/plain"); }
            catch (const Failure& error) { failure(nested, error); }
        } else if (type.token.starts_with("text/")) {
            auto charset = parameter_value(reader, type, "charset", packet);
            if (charset.empty()) { charset = "us-ascii"; }
            extracted.decoded("body_charset", ValueKind::DecodedBytes, charset, body_packet, 3, "ASCII");
            const auto text = decode_text(extracted, payload, charset);
            if (!text.supported) { throw Failure{ParseStatus::Unsupported, "Unsupported body charset; decoded bytes retained", 0}; }
            if (!text.valid) { throw Failure{ParseStatus::Malformed, "Invalid body character encoding; decoded bytes retained", 0}; }
            extracted.decoded("body_text", ValueKind::Text, text.value, body_packet);
        } else if (type.token == "application/pkcs7-mime" || type.token == "application/x-pkcs7-mime" ||
                   type.token == "application/pkcs7-signature" || type.token == "application/pgp-encrypted" || type.token == "application/pgp-signature") {
            extracted.decoded("mime_security_verification", ValueKind::Text, "not_performed", body_packet, 0);
        }
    }
public:
    ParseStatus run(Reader& reader, const std::span<const std::uint8_t> bytes) {
        try { part(reader, bytes, "email/message0", 1, true, "text/plain"); }
        catch (const Failure& error) { failure(reader, error); }
        reader.derived("part_count", parts); reader.derived("message_count", messages);
        reader.derived("attachment_count", attachments); reader.derived("received_route_count", routes);
        return status;
    }
};
}
}

namespace metaxtract::native {
namespace {
class EmailParser final : public Parser {
    const ParserCapability& capability() const noexcept override {
        static const ParserCapability value{"email.mime", "EML headers and MIME content", "1", Extract | EnumerateChildren, {"eml", "mime"}};
        return value;
    }
    ParseStatus run(ParserContext& context) const override {
        formats::Reader reader(context, 0, context.source_size(), "email.mime");
        try {
            const auto bytes = reader.read(0, reader.size);
            return formats::Email().run(reader, bytes);
        } catch (const formats::Failure& error) {
            reader.diagnostic("failure", error.message, error.offset);
            return error.status;
        }
    }
};
}
std::unique_ptr<Parser> email_mime_parser() { return std::make_unique<EmailParser>(); }
}
