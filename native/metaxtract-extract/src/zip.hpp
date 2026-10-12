#pragma once

#include "format.hpp"

namespace metaxtract::native::formats {

struct ZipEntry final {
    std::string name;
    std::uint64_t central_offset = 0;
    std::uint64_t local_offset = 0;
    std::uint64_t data_offset = 0;
    std::uint64_t compressed_size = 0;
    std::uint64_t size = 0;
    std::uint32_t crc = 0;
    std::uint16_t flags = 0;
    std::uint16_t compression = 0;
    bool zip64_sizes = false;
    std::uint64_t index = 0;
};

std::vector<ZipEntry> zip_directory(Reader& source);
std::vector<std::uint8_t> zip_payload(Reader& source, const ZipEntry& entry);
bool unsafe_member_name(std::string_view name);

}
