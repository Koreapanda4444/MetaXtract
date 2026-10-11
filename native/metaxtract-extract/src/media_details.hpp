#pragma once
#include "format.hpp"
namespace metaxtract::native::formats {
void inspect_media_sample(Reader&, const std::string&, std::uint32_t = 0);
bool inspect_media_box(Reader&, const std::string&, std::uint32_t = 0);
void inspect_transport_descriptors(Reader&);
void inspect_mpeg_timestamps(Reader&, std::uint64_t, bool = false);
void inspect_ebml_block(Reader&, bool);
void inspect_avi_format(Reader&, const std::string&);
void inspect_id3_details(Reader&, const std::string&, unsigned, std::uint32_t = 0);
void inspect_flac_picture(Reader&);
}
