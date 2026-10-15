#pragma once
#include "format.hpp"
#include <qpdf/QPDF.hh>
#include <qpdf/QPDFObjectHandle.hh>
#include <map>
#include <set>

namespace metaxtract::native::formats::pdf {
struct XrefSlot {
    unsigned type = 0;
    std::uint64_t offset = 0;
    std::uint64_t generation = 0;
    std::uint64_t object_stream = 0;
    std::uint64_t object_index = 0;
};
struct Revision {
    std::uint64_t offset = 0;
    std::uint64_t dictionary_offset = 0;
    std::uint64_t dictionary_length = 0;
    bool stream = false;
    QPDFObjectHandle trailer;
};
struct History {
    std::vector<Revision> revisions;
    std::map<std::uint64_t, XrefSlot> slots;
    std::uint64_t eof_end = 0;
};
class Cursor final {
    Reader& r_;
    std::uint64_t cached_at_ = 0;
    std::vector<std::uint8_t> cached_;
public:
    std::uint64_t position;
    Cursor(Reader& r, std::uint64_t at);
    int peek();
    int next();
    void space(bool comments = true);
    std::string word();
    std::uint64_t decimal();
    std::vector<std::uint8_t> dictionary();
};
void depth(Reader& r, std::uint32_t level);
void value(Reader& r, const std::string& key, ValueKind kind, std::string data, const std::string& stream);
void number(Reader& r, const std::string& key, std::int64_t data, const std::string& stream);
std::string object_id(QPDFObjectHandle object);
void scalar_tree(Reader& r, QPDFObjectHandle object, const std::string& key,
                 const std::string& stream, std::uint32_t level = 0);
std::vector<std::uint8_t> stream_bytes(Reader& r, QPDFObjectHandle stream, bool decode);
QPDFObjectHandle raw_dictionary(Reader& r, QPDF& raw, std::uint64_t offset, std::uint64_t object, std::uint64_t generation);
History history(Reader& r, QPDF& raw);
using SecurityInspector = void (*)(QPDF*, QPDF&, Reader&, const History&);
ParseStatus inspect(ParserContext& context, SecurityInspector security = nullptr, bool preview = false);
}
