#include "media_details.hpp"
#include "boxes.hpp"
#include <bit>
#include <cmath>
#include <set>

namespace metaxtract::native::formats {
namespace {
void text(Reader& r, const std::string& key, std::uint64_t at, std::uint64_t count, const std::string& encoding = "UTF-8") {
    auto bytes = r.read(at, count);
    std::string value(bytes.begin(), bytes.end());
    r.put(key, ValueKind::Text, std::move(value), at, std::move(bytes), encoding);
}
std::uint64_t terminated(Reader& r, std::uint64_t at, unsigned width = 1) {
    r.require(at, 0); const auto begin = at;
    while (width <= r.size - at) {
        r.checkpoint(); const auto bytes = r.read(at, width); at += width;
        if (bytes[0] == 0 && (width == 1 || bytes[1] == 0)) { return at - begin - width; }
    }
    throw Failure{ParseStatus::Malformed, "Unterminated media metadata string", r.base + begin};
}
void payload(Reader& r, const std::string& key, std::uint64_t at, std::uint64_t count) { r.put(key, ValueKind::Bytes, {}, at, r.read(at, count)); }
void signed_number(Reader& r, const std::string& key, std::uint64_t at, unsigned width, bool little = false) {
    auto bytes = r.read(at, width); auto bits = integer(bytes, 0, width, little);
    if (width < 8 && (bits & (1ULL << (8U * width - 1U)))) { bits |= ~((1ULL << (8U * width)) - 1U); }
    r.put(key, ValueKind::Signed, std::to_string(std::bit_cast<std::int64_t>(bits)), at, std::move(bytes));
}
void descriptors(Reader& r, std::uint64_t begin, std::uint64_t end, std::uint32_t depth) {
    if (!r.context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "Codec descriptor depth exceeded", r.base}; }
    for (auto at = begin; at < end;) {
        r.entry(); const auto tag = r.read(at++, 1)[0]; std::uint64_t length = 0; bool ended = false;
        for (unsigned n = 0; n < 4; ++n) { if (at >= end) { throw Failure{ParseStatus::Malformed, "Truncated MPEG-4 descriptor size", r.base + at}; } const auto byte = r.read(at++, 1)[0]; length = (length << 7U) | (byte & 127U); if (!(byte & 128U)) { ended = true; break; } }
        if (!ended || length > end - at) { throw Failure{ParseStatus::Malformed, "Codec descriptor escapes container", r.base + at}; }
        auto d = r.window(at, length, r.group + ".descriptor");
        d.derived("descriptor_tag", tag); payload(d, "descriptor_payload", 0, length);
        if (tag == 4) {
            d.require(0, 13); d.number("object_type_indication", 0, 1); d.number("stream_type_flags", 1, 1);
            d.number("max_bitrate", 5, 4); d.number("average_bitrate", 9, 4); descriptors(d, 13, length, depth + 1);
        } else if (tag == 3) {
            d.require(0, 3); const auto flags = d.read(2, 1)[0]; std::uint64_t next = 3;
            if (flags & 128U) { next += 2; }
            if (flags & 64U) { next += 1 + d.read(next, 1)[0]; }
            if (flags & 32U) { next += 2; }
            d.require(0, next); descriptors(d, next, length, depth + 1);
        }
        at += length;
    }
}
void codec_boxes(Reader& r, std::uint64_t at, std::uint32_t level) {
    unsigned index = 0;
    walk_boxes(r, at, r.size, [&](const Box& box) {
        auto child = r.window(box.offset + box.header, box.size - box.header, r.group + "." + path_component(box.type) + std::to_string(index++));
        if (!inspect_media_box(child, box.type, level + 1)) { payload(child, "codec_extension", 0, child.size); }
    });
}
}

bool inspect_media_box(Reader& r, const std::string& type, std::uint32_t level) {
    if (!r.context.budget().check_depth(level)) { throw Failure{ParseStatus::Internal, "Media detail depth exceeded", r.base}; }
    if (type == "avcC") {
        r.require(0, 7); r.number("avc_configuration_version", 0, 1); r.number("avc_profile", 1, 1); r.number("avc_compatibility", 2, 1); r.number("avc_level", 3, 1);
        r.derived("nal_length_bytes", (r.read(4, 1)[0] & 3U) + 1); payload(r, "codec_configuration", 0, r.size);
        std::uint64_t at = 6;
        const auto sequence_count = r.read(5, 1)[0] & 31U;
        for (unsigned n = 0; n < sequence_count; ++n) { r.entry(); const auto length = integer(r.read(at, 2), 0, 2); at += 2; payload(r, "sequence_parameter_set", at, length); at += length; }
        const auto pictures = r.read(at++, 1)[0];
        for (unsigned n = 0; n < pictures; ++n) { r.entry(); const auto length = integer(r.read(at, 2), 0, 2); at += 2; payload(r, "picture_parameter_set", at, length); at += length; }
    } else if (type == "hvcC") {
        r.require(0, 23); r.number("hevc_configuration_version", 0, 1); r.derived("hevc_profile", r.read(1, 1)[0] & 31U);
        r.derived("hevc_tier", (r.read(1, 1)[0] >> 5U) & 1U); r.number("hevc_level", 12, 1);
        r.derived("nal_length_bytes", (r.read(21, 1)[0] & 3U) + 1); payload(r, "codec_configuration", 0, r.size);
        std::uint64_t at = 23; const auto arrays = r.read(22, 1)[0];
        for (unsigned n = 0; n < arrays; ++n) {
            r.entry(); const auto kind = r.read(at++, 1)[0] & 63U; const auto count = integer(r.read(at, 2), 0, 2); at += 2; r.derived("parameter_set_type", kind);
            for (std::uint64_t j = 0; j < count; ++j) { r.entry(); const auto length = integer(r.read(at, 2), 0, 2); at += 2; payload(r, "parameter_set", at, length); at += length; }
        }
    } else if (type == "av1C") {
        r.require(0, 4); const auto flags = r.read(0, 3); r.derived("av1_configuration_version", flags[0] & 127U);
        r.derived("av1_profile", flags[1] >> 5U); r.derived("av1_level", flags[1] & 31U);
        r.derived("av1_tier", flags[2] >> 7U); r.derived("av1_high_bit_depth", (flags[2] >> 6U) & 1U); payload(r, "codec_configuration", 0, r.size);
    } else if (type == "esds") { r.require(0, 4); descriptors(r, 4, r.size, level + 1); }
    else if (type == "colr") {
        text(r, "color_type", 0, 4, "ASCII"); const auto kind = r.read(0, 4);
        if (starts(kind, "nclx") || starts(kind, "nclc")) { r.number("primaries", 4, 2); r.number("transfer_characteristics", 6, 2); r.number("matrix_coefficients", 8, 2); if (starts(kind, "nclx")) { r.derived("full_range", r.read(10, 1)[0] >> 7U); } }
        else if (starts(kind, "prof") || starts(kind, "rICC")) { auto profile = r.window(4, r.size - 4, r.group + ".icc"); inspect_icc(profile); }
        else { payload(r, "unknown_color_profile", 4, r.size - 4); }
    } else if (type == "pasp") { r.number("pixel_aspect_horizontal", 0, 4); r.number("pixel_aspect_vertical", 4, 4); }
    else if (type == "clap") {
        r.require(0, 32); for (unsigned n = 0; n < 8; ++n) { r.number("clean_aperture_component_" + std::to_string(n), 4 * n, 4); }
    } else if (type == "chpl") {
        const auto version = r.read(0, 1)[0]; std::uint64_t at = version ? 8 : 4; const auto count = r.read(at++, 1)[0];
        for (unsigned n = 0; n < count; ++n) { r.entry(); r.number("chapter_start_100ns", at, 8); at += 8; const auto length = r.read(at++, 1)[0]; text(r, "chapter_title", at, length); at += length; }
    } else if (type == "chap") {
        if (r.size % 4) { throw Failure{ParseStatus::Malformed, "Incomplete chapter track reference", r.base}; }
        for (std::uint64_t at = 0; at < r.size; at += 4) { r.entry(); r.number("chapter_track_id", at, 4); }
    } else if (type == "elst") {
        const auto version = r.read(0, 1)[0]; if (version > 1) { throw Failure{ParseStatus::Unsupported, "Unknown edit-list version", r.base}; }
        const auto count = integer(r.read(4, 4), 0, 4); const auto width = version ? 8U : 4U; const auto stride = 2U * width + 4U;
        r.require(8, count * stride);
        for (std::uint64_t at = 8; at < 8 + count * stride; at += stride) { r.entry(); r.number("edit_duration_ticks", at, width); signed_number(r, "edit_media_time", at + width, width); signed_number(r, "edit_rate_integer", at + 2 * width, 2); r.number("edit_rate_fraction", at + 2 * width + 2, 2); }
    } else if (type == "keys") {
        const auto count = integer(r.read(4, 4), 0, 4); std::uint64_t at = 8;
        for (std::uint64_t n = 0; n < count; ++n) { r.entry(); const auto length = integer(r.read(at, 4), 0, 4); if (length < 8) { throw Failure{ParseStatus::Malformed, "Invalid QuickTime metadata key length", r.base + at}; } text(r, "key_namespace", at + 4, 4, "ASCII"); text(r, "key_name", at + 8, length - 8); at += length; }
        if (at != r.size) { r.diagnostic("unused_key_bytes", "QuickTime key table has trailing bytes", at); }
    } else if (type == "data") {
        r.require(0, 8); const auto kind = integer(r.read(0, 4), 0, 4) & 0xffffffU; r.number("data_type_flags", 0, 4); r.number("data_locale", 4, 4);
        const auto size = r.size - 8;
        if (kind == 1 || kind == 2) { text(r, "metadata_text", 8, size, kind == 1 ? "UTF-8" : "UTF-16BE"); }
        else if (kind == 13 || kind == 14) { r.decoded("cover_mime", ValueKind::Text, kind == 13 ? "image/jpeg" : "image/png", "bmff.data", 0); payload(r, "cover_data", 8, size); }
        else if (kind == 21 && size && size <= 8) { signed_number(r, "metadata_integer", 8, static_cast<unsigned>(size)); }
        else { payload(r, "metadata_data", 8, size); }
    } else { return false; }
    return true;
}

void inspect_media_sample(Reader& r, const std::string& codec, std::uint32_t level) {
    if (!r.context.budget().check_depth(level)) { throw Failure{ParseStatus::Internal, "Sample description depth exceeded", r.base}; }
    static const std::set<std::string> video{"avc1","avc3","hvc1","hev1","av01","vp08","vp09","jpeg","mjpa","mjpb","mp4v","encv"};
    static const std::set<std::string> audio{"mp4a","alac","Opus","fLaC","ac-3","ec-3","lpcm","sowt","twos","in24","in32","fl32","fl64","enca"};
    static const std::set<std::string> subtitle{"tx3g","stpp","wvtt","c608","c708","text"};
    if (video.contains(codec)) {
        r.require(0, 78); r.number("coded_width", 24, 2); r.number("coded_height", 26, 2); r.number("frame_count", 40, 2); r.number("coded_depth", 74, 2);
        const auto count = r.read(42, 1)[0]; if (count > 31) { throw Failure{ParseStatus::Malformed, "Invalid compressor name length", r.base + 42}; }
        text(r, "compressor_name", 43, count, "unspecified"); codec_boxes(r, 78, level);
    } else if (audio.contains(codec)) {
        r.require(0, 28); const auto version = integer(r.read(8, 2), 0, 2); r.number("sound_description_version", 8, 2);
        r.number("channels", 16, 2); r.number("bits_per_sample", 18, 2);
        auto bytes = r.read(24, 4); const auto rate = static_cast<double>(integer(bytes, 0, 4)) / 65536.0; r.put("sample_rate", ValueKind::Float, std::to_string(rate), 24, bytes);
        if (version == 0) { codec_boxes(r, 28, level); }
        else if (version == 1) { r.require(0, 44); r.number("samples_per_packet", 28, 4); r.number("bytes_per_packet", 32, 4); r.number("bytes_per_frame", 36, 4); codec_boxes(r, 44, level); }
        else { r.diagnostic("extended_sound_description", "Extended QuickTime sound description retained without guessing its layout", 8); }
    } else if (subtitle.contains(codec)) { r.decoded("subtitle_codec", ValueKind::Text, codec, "bmff.sample", 0, "ASCII"); payload(r, "subtitle_configuration", 0, r.size); }
}

void inspect_transport_descriptors(Reader& reader) {
    for (std::uint64_t at = 0; at < reader.size;) {
        reader.entry(); const auto h = reader.read(at, 2); auto r = reader.window(at + 2, h[1], reader.group + ".descriptor");
        reader.number("descriptor_tag", at, 1); payload(r, "descriptor_data", 0, r.size);
        if (h[0] == 5) { text(r, "registration_format", 0, 4, "ASCII"); }
        else if (h[0] == 10) {
            if (r.size % 4) { throw Failure{ParseStatus::Malformed, "Incomplete ISO language descriptor", r.base}; }
            for (std::uint64_t pos = 0; pos < r.size; pos += 4) { r.entry(); text(r, "language", pos, 3, "ASCII"); r.number("audio_type", pos + 3, 1); }
        } else if (h[0] == 0x59) {
            if (r.size % 8) { throw Failure{ParseStatus::Malformed, "Incomplete DVB subtitle descriptor", r.base}; }
            for (std::uint64_t pos = 0; pos < r.size; pos += 8) { r.entry(); text(r, "subtitle_language", pos, 3, "ASCII"); r.number("subtitling_type", pos + 3, 1); r.number("composition_page_id", pos + 4, 2); r.number("ancillary_page_id", pos + 6, 2); }
        } else if (h[0] == 0x6a || h[0] == 0x7a) { r.decoded("descriptor_codec", ValueKind::Text, h[0] == 0x6a ? "ac3" : "eac3", "ts.descriptor", 0); }
        at += 2 + h[1];
    }
}

void inspect_mpeg_timestamps(Reader& r, std::uint64_t begin, bool fragment) {
    r.require(begin, 3); const auto header = r.read(begin, 3);
    if ((header[0] & 0xc0U) != 0x80U) { return; }
    if (header[2] > r.size - begin - 3 && fragment) { r.diagnostic("fragmented_pes_header", "PES header continues in another transport packet", begin); return; }
    r.require(begin + 3, header[2]); const auto flags = header[1] >> 6U;
    if (flags == 1 || ((flags == 2 || flags == 3) && header[2] < (flags == 3 ? 10U : 5U))) { throw Failure{ParseStatus::Malformed, "Invalid PES timestamp flags or length", r.base + begin}; }
    for (unsigned n = 0; n < (flags == 3 ? 2U : flags == 2 ? 1U : 0U); ++n) {
        const auto at = begin + 3 + 5 * n; auto bytes = r.read(at, 5);
        if (!(bytes[0] & 1U) || !(bytes[2] & 1U) || !(bytes[4] & 1U)) { throw Failure{ParseStatus::Malformed, "PES timestamp marker missing", r.base + at}; }
        const std::uint64_t value = (static_cast<std::uint64_t>((bytes[0] >> 1U) & 7U) << 30U) | (static_cast<std::uint64_t>(integer(bytes, 1, 2) >> 1U) << 15U) | (integer(bytes, 3, 2) >> 1U);
        r.put(n ? "dts_90khz" : "pts_90khz", ValueKind::Unsigned, std::to_string(value), at, std::move(bytes));
    }
}

void inspect_ebml_block(Reader& r, bool simple) {
    const auto first = r.read(0, 1)[0]; unsigned width = 1; unsigned mask = 128;
    while (width <= 8 && !(first & mask)) { ++width; mask >>= 1; }
    if (width > 8) { throw Failure{ParseStatus::Malformed, "Invalid Matroska block track number", r.base}; }
    auto track_bytes = r.read(0, width); std::uint64_t track = first & (mask - 1U);
    for (unsigned n = 1; n < width; ++n) { track = (track << 8U) | track_bytes[n]; }
    r.put("block_track_number", ValueKind::Unsigned, std::to_string(track), 0, track_bytes); signed_number(r, "block_timecode", width, 2);
    const auto flags = r.read(width + 2, 1)[0]; const auto lacing = (flags >> 1U) & 3U; auto at = static_cast<std::uint64_t>(width + 3);
    r.derived("lacing_mode", lacing); if (simple) { r.derived("keyframe", flags >> 7U); r.derived("discardable", flags & 1U); }
    r.derived("invisible", (flags >> 3U) & 1U);
    const auto count = lacing ? static_cast<unsigned>(r.read(at++, 1)[0]) + 1U : 1U;
    r.derived("frame_count", count);
    if (!lacing) { r.derived("frame_offset", r.base + at); r.derived("frame_bytes", r.size - at); }
    else {
        payload(r, "laced_block_prefix", width + 3, std::min<std::uint64_t>(r.size - width - 3, 1024));
        r.diagnostic("laced_frames", "Laced block framing retained; per-frame boundaries require native demuxing", at);
    }
}

void inspect_avi_format(Reader& r, const std::string& kind) {
    if (kind == "vids") {
        r.require(0, 40); signed_number(r, "stored_width", 4, 4, true); signed_number(r, "stored_height", 8, 4, true);
        r.number("planes", 12, 2, true); r.number("bits_per_pixel", 14, 2, true); text(r, "compression_fourcc", 16, 4, "ASCII");
    } else if (kind == "auds") {
        r.require(0, 16); r.number("format_code", 0, 2, true); r.number("channels", 2, 2, true);
        r.number("sample_rate", 4, 4, true); r.number("block_align", 12, 2, true); r.number("bits_per_sample", 14, 2, true);
    }
}

void inspect_flac_picture(Reader& r) {
    r.number("picture_type", 0, 4); const auto mime_length = integer(r.read(4, 4), 0, 4); text(r, "cover_mime", 8, mime_length, "ASCII");
    auto at = 8 + mime_length; const auto description = integer(r.read(at, 4), 0, 4); at += 4; text(r, "cover_description", at, description); at += description;
    r.number("cover_width", at, 4); r.number("cover_height", at + 4, 4); r.number("cover_bits_per_pixel", at + 8, 4); r.number("cover_palette_colors", at + 12, 4);
    const auto length = integer(r.read(at + 16, 4), 0, 4); at += 20; payload(r, "cover_data", at, length);
    if (at + length != r.size) { r.diagnostic("trailing_picture_data", "Bytes remain after FLAC picture payload", at + length); }
}

void inspect_id3_details(Reader& r, const std::string& id, unsigned version, std::uint32_t level) {
    if (!r.context.budget().check_depth(level)) { throw Failure{ParseStatus::Internal, "ID3 chapter nesting limit exceeded", r.base}; }
    if (id == "APIC" || id == "PIC") {
        const auto encoding = r.read(0, 1)[0]; std::uint64_t at = 1;
        if (id == "PIC") { text(r, "cover_format", at, 3, "ASCII"); at += 3; }
        else { const auto length = terminated(r, at); text(r, "cover_mime", at, length, "ASCII"); at += length + 1; }
        r.number("picture_type", at++, 1);
        const auto width = encoding == 1 || encoding == 2 ? 2U : 1U; const auto count = terminated(r, at, width);
        text(r, "cover_description", at, count, encoding == 0 ? "ISO-8859-1" : encoding == 3 ? "UTF-8" : encoding == 2 ? "UTF-16BE" : "UTF-16");
        at += count + width; payload(r, "cover_data", at, r.size - at);
    } else if (id == "CHAP" || id == "CTOC") {
        const auto count = terminated(r, 0); text(r, "chapter_element_id", 0, count); auto at = count + 1;
        if (id == "CHAP") {
            r.number("chapter_start_ms", at, 4); r.number("chapter_end_ms", at + 4, 4); r.number("chapter_start_byte", at + 8, 4); r.number("chapter_end_byte", at + 12, 4); at += 16;
        } else {
            r.number("toc_flags", at++, 1); const auto children = r.read(at++, 1)[0];
            for (unsigned n = 0; n < children; ++n) { r.entry(); const auto length = terminated(r, at); text(r, "chapter_child_id", at, length); at += length + 1; }
        }
        while (at < r.size) {
            r.entry(); const auto h = r.read(at, 10); if (!h[0]) { break; }
            std::uint64_t length = 0; if (version == 4) { for (unsigned n = 4; n < 8; ++n) { if (h[n] & 128U) { throw Failure{ParseStatus::Malformed, "Invalid nested ID3 size", r.base + at}; } length = (length << 7U) | h[n]; } } else { length = integer(h, 4, 4); }
            const std::string name(h.begin(), h.begin() + 4); auto frame = r.window(at + 10, length, r.group + ".chapter_frame");
            if (h[8] || h[9]) { payload(frame, "encoded_chapter_frame", 0, length); }
            else if (name == "TIT2" && length > 1 && frame.read(0, 1)[0] == 3) { text(frame, "chapter_title", 1, length - 1); }
            else { payload(frame, "chapter_frame", 0, length); inspect_id3_details(frame, name, version, level + 1); }
            at += 10 + length;
        }
    }
}
}
