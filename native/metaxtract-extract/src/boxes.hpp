#pragma once

#include "format.hpp"

namespace metaxtract::native::formats {

struct Box final {
    std::uint64_t offset;
    std::uint64_t header;
    std::uint64_t size;
    std::string type;
};

template<typename Visitor>
void walk_boxes(Reader& reader, std::uint64_t begin, std::uint64_t end, Visitor&& visitor) {
    reader.require(begin, end >= begin ? end - begin : reader.size + 1);
    while (begin < end) {
        reader.entry();
        if (end - begin < 8) { throw Failure{ParseStatus::Malformed, "Incomplete box header", reader.base + begin}; }
        const auto data = reader.read(begin, 8);
        auto length = integer(data, 0, 4);
        std::uint64_t header = 8;
        if (length == 1) {
            if (end - begin < 16) { throw Failure{ParseStatus::Malformed, "Incomplete extended box header", reader.base + begin}; }
            length = integer(reader.read(begin + 8, 8), 0, 8); header = 16;
        } else if (length == 0) { length = end - begin; }
        const std::string type(data.begin() + 4, data.end());
        if (type == "uuid") { header += 16; }
        if (length < header || length > end - begin) { throw Failure{ParseStatus::Malformed, "Box exceeds its enclosing container", reader.base + begin}; }
        visitor(Box{begin, header, length, type});
        begin += length;
    }
}

}
