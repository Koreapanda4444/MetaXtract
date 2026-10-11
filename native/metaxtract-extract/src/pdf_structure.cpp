#include "pdf.hpp"
#include <qpdf/Pl_Function.hh>
#include <cctype>

namespace metaxtract::native::formats::pdf {
namespace {
bool white(int c) { return c == 0 || c == 9 || c == 10 || c == 12 || c == 13 || c == 32; }
bool delimiter(int c) { return white(c) || c == '(' || c == ')' || c == '<' || c == '>' || c == '[' || c == ']' || c == '{' || c == '}' || c == '/' || c == '%'; }
void bad(Reader& r, const std::string& message, std::uint64_t at) { throw Failure{ParseStatus::Malformed, message, std::min(at, r.size)}; }
std::string text(const std::vector<std::uint8_t>& data) { return {data.begin(), data.end()}; }
}
void depth(Reader& r, std::uint32_t level) {
    r.checkpoint();
    if (!r.context.budget().check_depth(level)) { throw Failure{ParseStatus::Internal, "PDF nesting limit exceeded", r.base}; }
}
void value(Reader& r, const std::string& key, ValueKind kind, std::string data, const std::string& stream) {
    if (!r.context.budget().reserve_allocation(data.size())) { throw Failure{ParseStatus::Internal, "PDF value allocation limit exceeded", r.base}; }
    r.decoded(key, kind, std::move(data), stream, 2, kind == ValueKind::DecodedBytes ? "" : "UTF-8");
}
void number(Reader& r, const std::string& key, std::int64_t data, const std::string& stream) { value(r, key, ValueKind::Signed, std::to_string(data), stream); }
std::string object_id(QPDFObjectHandle object) {
    const auto id = object.getObjGen();
    return "pdf.object." + std::to_string(id.getObj()) + "." + std::to_string(id.getGen());
}
Cursor::Cursor(Reader& r, std::uint64_t at) : r_(r), position(at) { r.require(at, 0); }
int Cursor::peek() {
    if (position >= r_.size) { return -1; }
    if (cached_.empty() || position < cached_at_ || position - cached_at_ >= cached_.size()) {
        cached_at_ = position; cached_ = r_.read(position, std::min<std::uint64_t>(4096, r_.size - position));
    }
    return cached_[static_cast<std::size_t>(position - cached_at_)];
}
int Cursor::next() {
    if ((position & 255U) == 0) { r_.checkpoint(); }
    const auto c = peek(); if (c >= 0) { ++position; } return c;
}
void Cursor::space(bool comments) {
    while (true) {
        while (white(peek())) { next(); }
        if (!comments || peek() != '%') { return; }
        r_.entry();
        while (peek() >= 0 && peek() != '\r' && peek() != '\n') { next(); }
    }
}
std::string Cursor::word() {
    space(); std::string result;
    while (peek() >= 0 && !delimiter(peek())) {
        if (result.size() == 128) { bad(r_, "PDF structural token is too long", position); }
        result.push_back(static_cast<char>(next()));
    }
    if (result.empty()) { bad(r_, "PDF structural token missing", position); }
    return result;
}
std::uint64_t Cursor::decimal() {
    space(); std::uint64_t result = 0; unsigned count = 0;
    while (peek() >= '0' && peek() <= '9') {
        const auto digit = static_cast<unsigned>(next() - '0');
        if (result > (UINT64_MAX - digit) / 10 || ++count > 20) { bad(r_, "PDF integer exceeds supported range", position); }
        result = result * 10 + digit;
    }
    if (!count || (peek() >= 0 && !delimiter(peek()))) { bad(r_, "PDF decimal integer is invalid", position); }
    return result;
}
std::vector<std::uint8_t> Cursor::dictionary() {
    space(); const auto start = position;
    if (next() != '<' || next() != '<') { bad(r_, "PDF dictionary opener missing", start); }
    std::vector<char> containers{'d'}; depth(r_, 1);
    while (!containers.empty()) {
        const auto c = next();
        if (c < 0) { bad(r_, "PDF dictionary is truncated", start); }
        if (c == '%') { while (peek() >= 0 && peek() != '\r' && peek() != '\n') { next(); } }
        else if (c == '(') {
            unsigned nesting = 1;
            while (nesting) {
                depth(r_, static_cast<std::uint32_t>(containers.size()) + nesting);
                const auto v = next(); if (v < 0) { bad(r_, "PDF literal string is truncated", start); }
                if (v == '\\') { if (next() < 0) { bad(r_, "PDF string escape is truncated", start); } }
                else if (v == '(') { ++nesting; }
                else if (v == ')') { --nesting; }
            }
        } else if (c == '<') {
            if (peek() == '<') { next(); containers.push_back('d'); r_.entry(); depth(r_, static_cast<std::uint32_t>(containers.size())); }
            else { while (peek() >= 0 && peek() != '>') { next(); } if (next() != '>') { bad(r_, "PDF hex string is truncated", start); } }
        } else if (c == '>') {
            if (next() != '>' || containers.back() != 'd') { bad(r_, "PDF dictionary closer is invalid", position); }
            containers.pop_back();
        } else if (c == '[') { containers.push_back('a'); r_.entry(); depth(r_, static_cast<std::uint32_t>(containers.size())); }
        else if (c == ']') {
            if (containers.back() != 'a') { bad(r_, "PDF array closer is invalid", position); }
            containers.pop_back();
        } else if (c == ')' || c == '{' || c == '}') { bad(r_, "Unexpected PDF structural delimiter", position); }
    }
    return r_.read(start, position - start);
}
QPDFObjectHandle raw_dictionary(Reader& r, QPDF& raw, std::uint64_t offset, std::uint64_t object, std::uint64_t generation) {
    Cursor cursor(r, offset);
    if (cursor.decimal() != object || cursor.decimal() != generation || cursor.word() != "obj") { bad(r, "PDF object header disagrees with cross-reference entry", offset); }
    cursor.space(); const auto at = cursor.position; const auto data = cursor.dictionary();
    if (!r.context.budget().reserve_allocation(data.size() * 4ULL)) { throw Failure{ParseStatus::Internal, "PDF object parse allowance exceeded", at}; }
    Reader located(r.context, 0, r.size, "pdf.raw.object" + std::to_string(object));
    located.put("object_dictionary_bytes", ValueKind::Bytes, "", at, data);
    return QPDFObjectHandle::parse(&raw, text(data), "raw PDF dictionary");
}
std::vector<std::uint8_t> stream_bytes(Reader& r, QPDFObjectHandle stream, bool decode) {
    if (!stream.isStream()) { bad(r, "PDF attachment or metadata is not a stream", r.base); }
    std::vector<std::uint8_t> data;
    Pl_Function output("metaxtract bounded PDF stream", nullptr, [&](const unsigned char* bytes, std::size_t length) {
        r.checkpoint();
        if (!r.context.budget().consume_decoded(length) || !r.context.budget().reserve_allocation(static_cast<std::uint64_t>(length) * 2)) {
            throw Failure{ParseStatus::Internal, "PDF stream output limit exceeded", r.base};
        }
        data.insert(data.end(), bytes, bytes + length);
    });
    bool filtered = false;
    const auto success = stream.pipeStreamData(&output, &filtered, 0, decode ? qpdf_dl_specialized : qpdf_dl_none, true, false);
    r.checkpoint();
    if (!success) { throw Failure{ParseStatus::Unsupported, "PDF stream filter or stream data could not be decoded completely", r.base}; }
    return data;
}
namespace {
void remember(History& h, std::uint64_t object, XrefSlot slot, bool hybrid) {
    const auto old = h.slots.find(object);
    if (old == h.slots.end() || (hybrid && old->second.type == 0 && slot.type == 2)) { h.slots[object] = slot; }
}
void stream_slots(Reader& r, QPDF& raw, History& h, Revision& revision, Cursor& cursor, bool hybrid) {
    auto dictionary = revision.trailer;
    const auto length_object = dictionary.getKey("/Length");
    if (length_object.isIndirect() || !length_object.isInteger() || length_object.getIntValue() < 0) {
        r.diagnostic("xref_stream_length_unavailable", "Cross-reference stream requires library resolution of its length; raw slot decoding is unavailable", revision.dictionary_offset);
        return;
    }
    if (cursor.word() != "stream") { bad(r, "Cross-reference stream keyword missing", cursor.position); }
    if (cursor.peek() == '\r') { cursor.next(); if (cursor.peek() == '\n') { cursor.next(); } }
    else if (cursor.peek() == '\n') { cursor.next(); }
    else { bad(r, "PDF stream newline missing", cursor.position); }
    const auto length = static_cast<std::uint64_t>(length_object.getIntValue()); r.require(cursor.position, length);
    const auto encoded = r.read(cursor.position, length);
    if (!r.context.budget().reserve_allocation(length * 3ULL)) { throw Failure{ParseStatus::Internal, "Cross-reference stream working allowance exceeded", cursor.position}; }
    auto stream = QPDFObjectHandle::newStream(&raw, text(encoded));
    stream.replaceDict(dictionary);
    const auto decoded = stream_bytes(r, stream, true);
    auto widths = dictionary.getKey("/W");
    if (!widths.isArray() || widths.getArrayNItems() != 3) { bad(r, "PDF cross-reference widths are invalid", revision.offset); }
    unsigned w[3]{};
    for (int i = 0; i < 3; ++i) {
        auto v = widths.getArrayItem(i);
        if (v.isIndirect() || !v.isInteger() || v.getIntValue() < 0 || v.getIntValue() > 8) { bad(r, "PDF cross-reference field width is unsupported", revision.offset); }
        w[i] = static_cast<unsigned>(v.getIntValue());
    }
    const auto stride = w[0] + w[1] + w[2]; if (!stride) { bad(r, "PDF cross-reference row width is zero", revision.offset); }
    std::vector<std::pair<std::uint64_t, std::uint64_t>> ranges;
    auto index = dictionary.getKey("/Index");
    if (index.isNull()) {
        auto size = dictionary.getKey("/Size");
        if (size.isIndirect() || !size.isInteger() || size.getIntValue() < 0) { bad(r, "PDF cross-reference size is invalid", revision.offset); }
        ranges.emplace_back(0, static_cast<std::uint64_t>(size.getIntValue()));
    } else {
        if (!index.isArray() || index.getArrayNItems() % 2) { bad(r, "PDF cross-reference index is invalid", revision.offset); }
        for (int i = 0; i < index.getArrayNItems(); i += 2) {
            r.entry(); auto first = index.getArrayItem(i), count = index.getArrayItem(i + 1);
            if (first.isIndirect() || count.isIndirect() || !first.isInteger() || !count.isInteger() || first.getIntValue() < 0 || count.getIntValue() < 0) { bad(r, "PDF cross-reference index contains invalid numbers", revision.offset); }
            ranges.emplace_back(static_cast<std::uint64_t>(first.getIntValue()), static_cast<std::uint64_t>(count.getIntValue()));
        }
    }
    std::size_t at = 0;
    for (const auto& [first, count] : ranges) {
        if (count > UINT64_MAX - first || count > (decoded.size() - at) / stride) { bad(r, "PDF cross-reference rows exceed decoded stream", revision.offset); }
        for (std::uint64_t i = 0; i < count; ++i) {
            r.entry(); const auto row = std::span<const std::uint8_t>(decoded).subspan(at, stride);
            const auto type = w[0] ? integer(row, 0, w[0]) : 1;
            const auto a = integer(row, w[0], w[1]), b = integer(row, w[0] + w[1], w[2]);
            if (type > 2) { r.diagnostic("unknown_xref_type", "Unknown cross-reference stream entry type retained", revision.offset); }
            else {
                XrefSlot slot; slot.type = static_cast<unsigned>(type);
                if (type == 2) { slot.object_stream = a; slot.object_index = b; }
                else { slot.offset = a; slot.generation = b; }
                remember(h, first + i, slot, hybrid);
            }
            at += stride;
        }
    }
    if (at != decoded.size()) { bad(r, "Cross-reference stream contains trailing row bytes", revision.offset); }
}
Revision parse_revision(Reader& r, QPDF& raw, History& h, std::uint64_t offset, bool hybrid) {
    Cursor cursor(r, offset); Revision result; result.offset = offset;
    const auto token = cursor.word();
    if (token == "xref") {
        while (true) {
            cursor.space();
            if (cursor.peek() == 't') { if (cursor.word() != "trailer") { bad(r, "PDF cross-reference trailer missing", cursor.position); } break; }
            const auto first = cursor.decimal(), count = cursor.decimal();
            if (count > UINT64_MAX - first) { bad(r, "PDF cross-reference subsection overflows", cursor.position); }
            for (std::uint64_t i = 0; i < count; ++i) {
                r.entry(); cursor.space(); const auto at = cursor.position;
                const auto location = cursor.decimal(), generation = cursor.decimal(); const auto flag = cursor.word();
                if ((flag != "n" && flag != "f") || generation > 65535 || (flag == "n" && location >= r.size)) { bad(r, "PDF cross-reference entry is invalid", at); }
                remember(h, first + i, {flag == "n" ? 1U : 0U, location, generation, 0, 0}, hybrid);
                r.put("xref_entry_bytes", ValueKind::Bytes, "", at, r.read(at, cursor.position - at));
            }
        }
    } else {
        Cursor header(r, offset); static_cast<void>(header.decimal()); static_cast<void>(header.decimal());
        if (header.word() != "obj") { bad(r, "Cross-reference stream object header missing", offset); }
        cursor.position = header.position; result.stream = true;
    }
    cursor.space(); result.dictionary_offset = cursor.position;
    const auto data = cursor.dictionary(); result.dictionary_length = data.size();
    r.put("trailer_dictionary_bytes", ValueKind::Bytes, "", result.dictionary_offset, data);
    if (!r.context.budget().reserve_allocation(data.size() * 4ULL)) { throw Failure{ParseStatus::Internal, "Trailer parse allowance exceeded", result.dictionary_offset}; }
    result.trailer = QPDFObjectHandle::parse(&raw, text(data), "PDF cross-reference trailer");
    if (!result.trailer.isDictionary()) { bad(r, "Cross-reference trailer is not a dictionary", offset); }
    if (result.stream) {
        auto type = result.trailer.getKey("/Type");
        if (type.isIndirect() || !type.isName() || type.getName() != "/XRef") { bad(r, "Cross-reference stream has no XRef type", offset); }
        stream_slots(r, raw, h, result, cursor, hybrid);
    }
    return result;
}
}
History history(Reader& r, QPDF& raw) {
    History result;
    const auto tail_at = r.size > 4096 ? r.size - 4096 : 0; const auto tail = text(r.read(tail_at, r.size - tail_at));
    const auto marker = tail.rfind("startxref");
    if (marker == std::string::npos) { bad(r, "Final PDF startxref marker missing", tail_at); }
    Cursor end(r, tail_at + marker + 9); end.space(false); const auto value_at = end.position;
    auto offset = end.decimal();
    r.put("startxref_offset", ValueKind::Unsigned, std::to_string(offset), value_at, r.read(value_at, end.position - value_at));
    end.space(false);
    if (text(r.read(end.position, std::min<std::uint64_t>(5, r.size - end.position))) != "%%EOF") { bad(r, "Final PDF EOF marker missing", end.position); }
    result.eof_end = end.position + 5;
    std::set<std::uint64_t> visited;
    while (true) {
        r.entry(); depth(r, 1);
        if (offset >= r.size || !visited.insert(offset).second) { bad(r, "PDF revision chain is cyclic or out of range", std::min(offset, r.size)); }
        Reader revision(r.context, 0, r.size, "pdf.revision" + std::to_string(result.revisions.size()));
        auto current = parse_revision(revision, raw, result, offset, false);
        const auto id = "pdf.revision" + std::to_string(result.revisions.size());
        number(revision, "xref_offset", static_cast<std::int64_t>(offset), id);
        value(revision, "xref_kind", ValueKind::Text, current.stream ? "stream" : "table", id);
        auto hybrid = current.trailer.getKey("/XRefStm");
        if (!hybrid.isNull()) {
            if (hybrid.isIndirect() || !hybrid.isInteger() || hybrid.getIntValue() < 0) { bad(r, "Hybrid cross-reference offset is invalid", offset); }
            const auto location = static_cast<std::uint64_t>(hybrid.getIntValue());
            if (!visited.insert(location).second) { bad(r, "Hybrid cross-reference offset is cyclic", location); }
            auto supplemental = parse_revision(revision, raw, result, location, true);
            if (!supplemental.stream) { bad(r, "Hybrid cross-reference supplement is not a stream", location); }
            number(revision, "hybrid_xref_offset", static_cast<std::int64_t>(location), id);
        }
        auto previous = current.trailer.getKey("/Prev");
        result.revisions.push_back(std::move(current));
        if (previous.isNull()) { break; }
        if (previous.isIndirect() || !previous.isInteger() || previous.getIntValue() < 0) { bad(r, "PDF previous-revision offset is invalid", offset); }
        const auto next = static_cast<std::uint64_t>(previous.getIntValue());
        number(revision, "previous_xref_offset", static_cast<std::int64_t>(next), id);
        offset = next;
    }
    number(r, "revision_count", static_cast<std::int64_t>(result.revisions.size()), "pdf.container");
    return result;
}
}
