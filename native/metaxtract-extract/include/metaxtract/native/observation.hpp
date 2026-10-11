#pragma once

#include <cstdint>
#include <string>
#include <vector>

namespace metaxtract::native {

enum class ValueKind : std::uint32_t {
    Bytes = 0, Unsigned = 1, Signed = 2, Float = 3, Text = 4, Rational = 5,
    UnsignedSequence = 6, SignedSequence = 7, FloatSequence = 8, RationalSequence = 9,
    DecodedBytes = 10,
};

struct Observation final {
    std::string name_space;
    std::string key;
    ValueKind kind = ValueKind::Bytes;
    std::string value;
    std::vector<std::uint8_t> encoded;
    std::string encoding;
    std::uint64_t offset = 0;
    std::uint64_t length = 0;
    bool located = false;
    std::uint64_t tag_code = 0;
    bool numeric_tag = false;
    std::uint32_t method = 0;
    std::string stream_id = {};
};

}
