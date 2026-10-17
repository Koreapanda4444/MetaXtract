#include "format.hpp"

#include <bit>
#include <memory>

namespace metaxtract::native::formats {
namespace {

std::uint32_t crc32(const std::span<const std::uint8_t> bytes, Reader& reader) {
    std::uint32_t crc = 0xffffffffU;
    std::size_t count = 0;
    for (const auto byte : bytes) {
        crc ^= byte;
        for (unsigned bit = 0; bit < 8; ++bit) { crc = (crc >> 1U) ^ ((crc & 1U) != 0 ? 0xedb88320U : 0U); }
        if (++count % 4096 == 0) { reader.checkpoint(); }
    }
    return crc;
}

std::uint64_t entropy_end(Reader& reader, std::uint64_t at) {
    bool prefix = false;
    std::uint64_t marker = at;
    while (at < reader.size) {
        reader.checkpoint();
        const auto bytes = reader.read(at, std::min<std::uint64_t>(65536, reader.size - at));
        for (std::size_t index = 0; index < bytes.size(); ++index) {
            const auto byte = bytes[index];
            if (!prefix) {
                if (byte == 255) { prefix = true; marker = at + index; }
            } else if (byte == 255) {
                continue;
            } else if (byte == 0 || (byte >= 0xd0 && byte <= 0xd7)) {
                prefix = false;
            } else {
                return marker;
            }
        }
        at += bytes.size();
    }
    throw Failure{ParseStatus::Malformed, "JPEG scan has no terminating marker", reader.base + reader.size};
}

std::uint64_t sub_blocks(Reader& reader, std::uint64_t at) {
    while (true) {
        reader.entry();
        const auto size = reader.read(at, 1)[0];
        ++at;
        if (size == 0) { return at; }
        reader.require(at, size);
        at += size;
    }
}

void inspect_dib(Reader& reader, const std::uint64_t at, const bool icon) {
    const auto header = reader.read(at, 4);
    const auto length = integer(header, 0, 4, true);
    if (length != 12 && length != 16 && length != 40 && length != 52 && length != 56 && length != 64 && length != 108 && length != 124) {
        throw Failure{ParseStatus::Malformed, "Unsupported DIB header size", reader.base + at};
    }
    auto bytes = reader.read(at, length);
    const auto core = length == 12;
    const auto width = core ? static_cast<std::int64_t>(integer(bytes, 4, 2, true)) :
        static_cast<std::int64_t>(std::bit_cast<std::int32_t>(static_cast<std::uint32_t>(integer(bytes, 4, 4, true))));
    const auto height = core ? static_cast<std::int64_t>(integer(bytes, 6, 2, true)) :
        static_cast<std::int64_t>(std::bit_cast<std::int32_t>(static_cast<std::uint32_t>(integer(bytes, 8, 4, true))));
    const auto planes_at = core ? 8U : 12U;
    const auto bits_at = core ? 10U : 14U;
    const auto planes = integer(bytes, planes_at, 2, true);
    const auto bits = integer(bytes, bits_at, 2, true);
    if (width <= 0 || height == 0 || planes != 1 || (bits != 1 && bits != 4 && bits != 8 && bits != 16 && bits != 24 && bits != 32)) {
        throw Failure{ParseStatus::Malformed, "Invalid DIB dimensions, planes, or bit depth", reader.base + at};
    }
    reader.number("dib_header_size", at, 4, true);
    reader.put("width", ValueKind::Unsigned, std::to_string(width), at + 4, reader.read(at + 4, core ? 2 : 4));
    reader.put("stored_height", ValueKind::Signed, std::to_string(height), at + (core ? 6 : 8), reader.read(at + (core ? 6 : 8), core ? 2 : 4));
    const auto absolute_height = static_cast<std::uint64_t>(height < 0 ? -height : height);
    if (icon && absolute_height % 2 != 0) { throw Failure{ParseStatus::Malformed, "ICO DIB height must include matching color and mask planes", reader.base + at}; }
    reader.derived("height", icon ? absolute_height / 2 : absolute_height);
    reader.derived("top_down", height < 0 ? 1 : 0);
    reader.number("bits_per_pixel", at + bits_at, 2, true);
    if (length >= 40) {
        reader.number("compression", at + 16, 4, true);
        reader.number("pixel_bytes", at + 20, 4, true);
        reader.number("x_pixels_per_meter", at + 24, 4, true);
        reader.number("y_pixels_per_meter", at + 28, 4, true);
        reader.number("palette_colors", at + 32, 4, true);
    }
    if (length >= 108) { reader.put("color_space", ValueKind::Bytes, "", at + 56, reader.read(at + 56, 4)); }
    if (length == 124) {
        reader.number("rendering_intent", at + 108, 4, true);
        const auto profile_offset = integer(bytes, 112, 4, true);
        const auto profile_size = integer(bytes, 116, 4, true);
        if (profile_size != 0) { reader.require(at + profile_offset, profile_size); }
        reader.number("profile_offset", at + 112, 4, true);
        reader.number("profile_size", at + 116, 4, true);
    }
}

}

void inspect_png(Reader& reader, const bool metadata) {
    if (!starts(reader.read(0, 8), std::string_view("\x89PNG\r\n\x1a\n", 8))) {
        throw Failure{ParseStatus::Malformed, "Invalid PNG signature", reader.base};
    }
    std::uint64_t at = 8;
    std::uint64_t count = 0;
    bool ihdr = false, idat = false, ended_idat = false, palette = false, ended = false;
    bool animation = false, default_in_animation = false, frame_data = false;
    std::uint64_t declared_frames = 0, controls = 0, next_sequence = 0, canvas_width = 0, canvas_height = 0;
    std::uint8_t color = 0;
    while (at < reader.size) {
        reader.entry();
        const auto header = reader.read(at, 8);
        const auto length = integer(header, 0, 4);
        for (std::size_t index = 4; index < 8; ++index) {
            if (!((header[index] >= 'A' && header[index] <= 'Z') || (header[index] >= 'a' && header[index] <= 'z'))) {
                throw Failure{ParseStatus::Malformed, "Invalid PNG chunk type", reader.base + at + 4};
            }
        }
        reader.require(at, length + 12);
        const std::string type(header.begin() + 4, header.end());
        auto body = reader.read(at + 4, length + 4);
        const auto checksum = reader.read(at + 8 + length, 4);
        const auto calculated = crc32(body, reader) ^ 0xffffffffU;
        if (calculated != integer(checksum, 0, 4)) {
            throw Failure{ParseStatus::Malformed, "PNG chunk CRC mismatch: " + type, reader.base + at};
        }
        Reader chunk(reader.context, reader.base, reader.size, reader.group + ".chunk" + std::to_string(count));
        chunk.put("type", ValueKind::Text, type, at + 4, reader.read(at + 4, 4), "ASCII");
        chunk.number("length", at, 4);
        if (metadata) { png_metadata(chunk, type, at + 8, length); }
        if (!ihdr && type != "IHDR") { throw Failure{ParseStatus::Malformed, "PNG IHDR must be first", reader.base + at}; }
        if (type == "IHDR") {
            if (ihdr || length != 13) { throw Failure{ParseStatus::Malformed, "Duplicate or malformed PNG IHDR", reader.base + at}; }
            const auto width = integer(body, 4, 4), height = integer(body, 8, 4);
            canvas_width = width; canvas_height = height;
            const auto bits = body[12];
            color = body[13];
            const bool depth = color == 0 ? (bits == 1 || bits == 2 || bits == 4 || bits == 8 || bits == 16) :
                color == 3 ? (bits == 1 || bits == 2 || bits == 4 || bits == 8) :
                (color == 2 || color == 4 || color == 6) && (bits == 8 || bits == 16);
            if (width == 0 || height == 0 || !depth || body[14] != 0 || body[15] != 0 || body[16] > 1) {
                throw Failure{ParseStatus::Malformed, "Invalid PNG dimensions, depth, color, or encoding fields", reader.base + at};
            }
            reader.number("width", at + 8, 4);
            reader.number("height", at + 12, 4);
            reader.number("bit_depth", at + 16, 1);
            reader.number("color_type", at + 17, 1);
            reader.number("interlace", at + 20, 1);
            ihdr = true;
        } else if (type == "acTL") {
            if (animation || idat || length != 8 || integer(body, 4, 4) == 0) {
                throw Failure{ParseStatus::Malformed, "Invalid APNG animation control", reader.base + at};
            }
            animation = true; declared_frames = integer(body, 4, 4);
            reader.number("animation_frame_count", at + 8, 4);
            reader.number("animation_play_count", at + 12, 4);
        } else if (type == "fcTL") {
            if (!animation || length != 26 || (controls && !frame_data) || integer(body, 4, 4) != next_sequence++) {
                throw Failure{ParseStatus::Malformed, "Invalid APNG frame control or sequence", reader.base + at};
            }
            const auto width = integer(body, 8, 4), height = integer(body, 12, 4);
            const auto x = integer(body, 16, 4), y = integer(body, 20, 4);
            if (!width || !height || width > canvas_width || height > canvas_height ||
                x > canvas_width - width || y > canvas_height - height || body[28] > 2 || body[29] > 1 ||
                (!idat && (x || y || width != canvas_width || height != canvas_height))) {
                throw Failure{ParseStatus::Malformed, "Invalid APNG frame geometry or operation", reader.base + at};
            }
            if (!controls) { default_in_animation = !idat; }
            ++controls; frame_data = false;
            chunk.number("sequence_number", at + 8, 4);
            chunk.number("frame_width", at + 12, 4); chunk.number("frame_height", at + 16, 4);
            chunk.number("x_offset", at + 20, 4); chunk.number("y_offset", at + 24, 4);
            chunk.number("delay_numerator", at + 28, 2); chunk.number("delay_denominator", at + 30, 2);
            chunk.number("dispose_operation", at + 32, 1); chunk.number("blend_operation", at + 33, 1);
            if (idat) { ended_idat = true; }
        } else if (type == "fdAT") {
            if (!animation || !idat || !controls || (default_in_animation && controls == 1) ||
                length < 4 || integer(body, 4, 4) != next_sequence++) {
                throw Failure{ParseStatus::Malformed, "Invalid APNG frame data or sequence", reader.base + at};
            }
            chunk.number("sequence_number", at + 8, 4);
            frame_data = true; ended_idat = true;
        } else if (type == "PLTE") {
            if (palette || idat || length == 0 || length % 3 != 0 || length > 768 || color == 0 || color == 4) {
                throw Failure{ParseStatus::Malformed, "Invalid PNG palette", reader.base + at};
            }
            palette = true;
        } else if (type == "IDAT") {
            if (ended_idat || (color == 3 && !palette)) { throw Failure{ParseStatus::Malformed, "Invalid PNG IDAT ordering", reader.base + at}; }
            idat = true;
            if (default_in_animation && controls == 1) { frame_data = true; }
        } else if (type == "IEND") {
            if (length != 0 || !idat) { throw Failure{ParseStatus::Malformed, "PNG IEND without image data or with payload", reader.base + at}; }
            ended = true;
            at += 12;
            ++count;
            break;
        } else {
            if (idat) { ended_idat = true; }
            if ((header[4] & 32U) == 0) { reader.diagnostic("unknown_critical_chunk", "Unknown critical PNG chunk: " + type, at); }
        }
        at += length + 12;
        ++count;
    }
    if (!ended) { throw Failure{ParseStatus::Malformed, "PNG IEND missing", reader.base + at}; }
    if (animation) {
        if (controls != declared_frames || !frame_data) {
            throw Failure{ParseStatus::Malformed, "APNG frame controls or data are incomplete", reader.base + at};
        }
        reader.derived("frame_count", declared_frames + (default_in_animation ? 0 : 1));
        reader.derived("default_image_in_animation", default_in_animation ? 1 : 0);
    }
    if (at != reader.size) { reader.diagnostic("trailing_data", "Bytes remain after PNG IEND", at); }
    reader.derived("chunk_count", count);
}

void inspect_jpeg(Reader& reader, const bool metadata) {
    if (!starts(reader.read(0, 2), std::string_view("\xff\xd8", 2))) {
        throw Failure{ParseStatus::Malformed, "JPEG SOI missing", reader.base};
    }
    std::uint64_t at = 2, segments = 0, scans = 0;
    bool dimensions = false, ended = false;
    std::vector<std::pair<std::uint8_t, std::vector<std::uint8_t>>> icc_parts;
    std::uint8_t icc_count = 0;
    while (at < reader.size) {
        reader.entry();
        auto marker = reader.read(at, 2);
        if (marker[0] != 255 || marker[1] == 0) { throw Failure{ParseStatus::Malformed, "Expected JPEG marker", reader.base + at}; }
        while (marker[1] == 255) { ++at; marker = reader.read(at, 2); }
        const auto code = marker[1];
        const auto marker_at = at;
        at += 2;
        Reader segment(reader.context, reader.base, reader.size, reader.group + ".segment" + std::to_string(segments));
        segment.number("marker", marker_at + 1, 1);
        ++segments;
        if (code == 0xd9) { ended = true; break; }
        if (code == 0xd8) { throw Failure{ParseStatus::Malformed, "Unexpected repeated JPEG SOI", reader.base + marker_at}; }
        if (code == 1 || (code >= 0xd0 && code <= 0xd7)) { continue; }
        const auto length = integer(reader.read(at, 2), 0, 2);
        if (length < 2) { throw Failure{ParseStatus::Malformed, "Invalid JPEG segment length", reader.base + at}; }
        reader.require(at, length);
        const auto payload = at + 2;
        if (metadata && ((code >= 0xe0 && code <= 0xef) || code == 0xfe)) { jpeg_metadata(segment, code, payload, length - 2); }
        if (metadata && code == 0xe2 && length >= 16) {
            const auto prefix = reader.read(payload, 14);
            if (starts(prefix, std::string_view("ICC_PROFILE\0", 12)) && prefix[13] > 1) {
                if (icc_count != 0 && icc_count != prefix[13]) { throw Failure{ParseStatus::Malformed, "Contradictory ICC part counts", reader.base + payload}; }
                icc_count = prefix[13];
                if (!reader.context.budget().reserve_allocation((length - 16) * 2 + 128)) { throw Failure{ParseStatus::Internal, "ICC fragment allocation", reader.base + payload}; }
                icc_parts.emplace_back(prefix[12], reader.read(payload + 14, length - 16));
            }
        }
        if (code >= 0xc0 && code <= 0xcf && code != 0xc4 && code != 0xc8 && code != 0xcc) {
            if (length < 8) { throw Failure{ParseStatus::Malformed, "Truncated JPEG frame header", reader.base + at}; }
            const auto body = reader.read(payload, 6);
            if (body[0] == 0 || integer(body, 3, 2) == 0 || body[5] == 0 || length != 8 + 3U * body[5]) {
                throw Failure{ParseStatus::Malformed, "Invalid JPEG frame component table", reader.base + payload};
            }
            segment.number("sample_precision", payload, 1);
            segment.number("height", payload + 1, 2);
            segment.number("width", payload + 3, 2);
            segment.number("components", payload + 5, 1);
            dimensions = true;
        }
        if (code == 0xda) {
            if (length < 6 || !dimensions) { throw Failure{ParseStatus::Malformed, "Invalid JPEG scan header", reader.base + at}; }
            const auto components = reader.read(payload, 1)[0];
            if (components == 0 || length != 6 + 2U * components) { throw Failure{ParseStatus::Malformed, "Invalid JPEG scan components", reader.base + at}; }
            ++scans;
            at = entropy_end(reader, at + length);
        } else {
            at += length;
        }
    }
    if (!ended || !dimensions || scans == 0) { throw Failure{ParseStatus::Malformed, "JPEG is missing frame, scan, or EOI", reader.base + at}; }
    if (at != reader.size) { reader.diagnostic("trailing_data", "Bytes remain after JPEG EOI", at); }
    if (icc_count != 0) {
        std::sort(icc_parts.begin(), icc_parts.end(), [](const auto& first, const auto& second) { return first.first < second.first; });
        if (icc_parts.size() != icc_count) { throw Failure{ParseStatus::Malformed, "Incomplete multipart ICC profile", reader.base}; }
        std::vector<std::uint8_t> profile;
        for (std::size_t index = 0; index < icc_parts.size(); ++index) {
            reader.checkpoint();
            if (icc_parts[index].first != index + 1) { throw Failure{ParseStatus::Malformed, "Duplicate or missing ICC fragment", reader.base}; }
            profile.insert(profile.end(), icc_parts[index].second.begin(), icc_parts[index].second.end());
        }
        Reader icc(reader.context, reader.base, reader.size, reader.group + ".icc"); inspect_icc_data(icc, profile);
    }
    reader.derived("segment_count", segments);
    reader.derived("scan_count", scans);
}

void inspect_webp(Reader& reader, const bool metadata) {
    const auto header = reader.read(0, 12);
    if (!starts(header, "RIFF") || !starts(header, "WEBP", 8)) { throw Failure{ParseStatus::Malformed, "Invalid WebP RIFF form", reader.base}; }
    const auto extent = integer(header, 4, 4, true) + 8;
    reader.require(0, extent);
    std::uint64_t at = 12, count = 0, frames = 0;
    bool dimensions = false;
    while (at < extent) {
        reader.entry();
        if (extent - at < 8) { throw Failure{ParseStatus::Malformed, "Incomplete WebP chunk header", reader.base + at}; }
        const auto chunk = reader.read(at, 8);
        const auto length = integer(chunk, 4, 4, true);
        const auto padded = length + (length % 2);
        if (padded > extent - at - 8) { throw Failure{ParseStatus::Malformed, "WebP chunk escapes RIFF", reader.base + at}; }
        const std::string type(chunk.begin(), chunk.begin() + 4);
        Reader field(reader.context, reader.base, reader.size, reader.group + ".chunk" + std::to_string(count));
        field.put("type", ValueKind::Text, type, at, reader.read(at, 4), "ASCII");
        const auto payload = at + 8;
        if (metadata) { webp_metadata(field, type, payload, length); }
        if (type == "VP8X") {
            if (length != 10) { throw Failure{ParseStatus::Malformed, "Invalid WebP VP8X length", reader.base + at}; }
            const auto body = reader.read(payload, 10);
            reader.put("width", ValueKind::Unsigned, std::to_string(integer(body, 4, 3, true) + 1), payload + 4, reader.read(payload + 4, 3));
            reader.put("height", ValueKind::Unsigned, std::to_string(integer(body, 7, 3, true) + 1), payload + 7, reader.read(payload + 7, 3));
            reader.number("feature_flags", payload, 1);
            dimensions = true;
        } else if (type == "VP8 ") {
            if (length < 10) { throw Failure{ParseStatus::Malformed, "Incomplete WebP VP8 frame", reader.base + at}; }
            const auto body = reader.read(payload, 10);
            if (!starts(body, std::string_view("\x9d\1\x2a", 3), 3) || (body[0] & 1U) != 0) { throw Failure{ParseStatus::Malformed, "Invalid WebP key frame", reader.base + payload}; }
            field.put("width", ValueKind::Unsigned, std::to_string(integer(body, 6, 2, true) & 0x3fffU), payload + 6, reader.read(payload + 6, 2));
            field.put("height", ValueKind::Unsigned, std::to_string(integer(body, 8, 2, true) & 0x3fffU), payload + 8, reader.read(payload + 8, 2));
            dimensions = true;
            ++frames;
        } else if (type == "VP8L") {
            if (length < 5) { throw Failure{ParseStatus::Malformed, "Incomplete WebP VP8L frame", reader.base + at}; }
            const auto body = reader.read(payload, 5);
            if (body[0] != 0x2f || (body[4] >> 5U) != 0) { throw Failure{ParseStatus::Malformed, "Invalid WebP lossless header", reader.base + payload}; }
            const auto packed = integer(body, 1, 4, true);
            field.put("width", ValueKind::Unsigned, std::to_string((packed & 0x3fffU) + 1), payload + 1, reader.read(payload + 1, 4));
            field.put("height", ValueKind::Unsigned, std::to_string(((packed >> 14U) & 0x3fffU) + 1), payload + 1, reader.read(payload + 1, 4));
            field.put("alpha", ValueKind::Unsigned, std::to_string((packed >> 28U) & 1U), payload + 1, reader.read(payload + 1, 4));
            dimensions = true;
            ++frames;
        } else if (type == "ANMF") {
            if (length < 16) { throw Failure{ParseStatus::Malformed, "Incomplete animated WebP frame", reader.base + at}; }
            const auto body = reader.read(payload, 16);
            field.put("width", ValueKind::Unsigned, std::to_string(integer(body, 6, 3, true) + 1), payload + 6, reader.read(payload + 6, 3));
            field.put("height", ValueKind::Unsigned, std::to_string(integer(body, 9, 3, true) + 1), payload + 9, reader.read(payload + 9, 3));
            field.number("duration_ms", payload + 12, 3, true);
            ++frames;
        }
        at += 8 + padded;
        ++count;
    }
    if (!dimensions || frames == 0) { throw Failure{ParseStatus::Malformed, "WebP image data missing", reader.base + at}; }
    if (extent != reader.size) { reader.diagnostic("trailing_data", "Bytes remain after WebP RIFF", extent); }
    reader.derived("chunk_count", count);
    reader.derived("frame_count", frames);
}

void inspect_gif(Reader& reader, const bool metadata) {
    const auto header = reader.read(0, 13);
    if ((!starts(header, "GIF87a") && !starts(header, "GIF89a")) || integer(header, 6, 2, true) == 0 || integer(header, 8, 2, true) == 0) {
        throw Failure{ParseStatus::Malformed, "Invalid GIF logical screen", reader.base};
    }
    reader.number("width", 6, 2, true);
    reader.number("height", 8, 2, true);
    reader.number("background_index", 11, 1);
    reader.number("pixel_aspect", 12, 1);
    std::uint64_t at = 13;
    if ((header[10] & 0x80U) != 0) { at += 3ULL * (1ULL << ((header[10] & 7U) + 1)); reader.require(0, at); }
    std::uint64_t frames = 0, extensions = 0;
    bool ended = false;
    while (at < reader.size) {
        reader.entry();
        const auto code = reader.read(at++, 1)[0];
        if (code == 0x3b) { ended = true; break; }
        if (code == 0x2c) {
            const auto body = reader.read(at, 9);
            if (integer(body, 4, 2, true) == 0 || integer(body, 6, 2, true) == 0) { throw Failure{ParseStatus::Malformed, "Empty GIF frame", reader.base + at}; }
            Reader frame(reader.context, reader.base, reader.size, reader.group + ".frame" + std::to_string(frames));
            frame.number("left", at, 2, true);
            frame.number("top", at + 2, 2, true);
            frame.number("width", at + 4, 2, true);
            frame.number("height", at + 6, 2, true);
            frame.number("flags", at + 8, 1);
            at += 9;
            if ((body[8] & 0x80U) != 0) { at += 3ULL * (1ULL << ((body[8] & 7U) + 1)); reader.require(0, at); }
            const auto minimum = reader.read(at++, 1)[0];
            if (minimum < 2 || minimum > 8) { throw Failure{ParseStatus::Malformed, "Invalid GIF LZW code size", reader.base + at - 1}; }
            at = sub_blocks(reader, at);
            ++frames;
        } else if (code == 0x21) {
            const auto label = reader.read(at++, 1)[0];
            Reader extension(reader.context, reader.base, reader.size, reader.group + ".extension" + std::to_string(extensions));
            extension.put("label", ValueKind::Unsigned, std::to_string(label), at - 1, reader.read(at - 1, 1));
            if (label == 0xf9) {
                const auto body = reader.read(at, 6);
                if (body[0] != 4 || body[5] != 0) { throw Failure{ParseStatus::Malformed, "Invalid GIF graphics control block", reader.base + at}; }
                extension.number("flags", at + 1, 1);
                extension.number("delay_centiseconds", at + 2, 2, true);
                extension.number("transparent_index", at + 4, 1);
            }
            const auto start = at;
            at = sub_blocks(reader, at);
            if (metadata) { gif_metadata(extension, label, start, at); }
            ++extensions;
        } else {
            throw Failure{ParseStatus::Malformed, "Unknown GIF block introducer", reader.base + at - 1};
        }
    }
    if (!ended || frames == 0) { throw Failure{ParseStatus::Malformed, "GIF image or trailer missing", reader.base + at}; }
    if (at != reader.size) { reader.diagnostic("trailing_data", "Bytes remain after GIF trailer", at); }
    reader.derived("frame_count", frames);
    reader.derived("extension_count", extensions);
}

void inspect_bmp(Reader& reader) {
    const auto header = reader.read(0, 14);
    if (!starts(header, "BM")) { throw Failure{ParseStatus::Malformed, "BMP signature missing", reader.base}; }
    const auto extent = integer(header, 2, 4, true);
    const auto pixels = integer(header, 10, 4, true);
    if (extent > reader.size || extent < 14 || pixels < 14 || pixels > extent) { throw Failure{ParseStatus::Malformed, "BMP file size or pixel offset invalid", reader.base}; }
    reader.number("file_size", 2, 4, true);
    reader.number("pixel_offset", 10, 4, true);
    inspect_dib(reader, 14, false);
    const auto dib_size = integer(reader.read(14, 4), 0, 4, true);
    if (pixels < 14 + dib_size) { throw Failure{ParseStatus::Malformed, "BMP pixels overlap the DIB header", reader.base + 10}; }
    const auto dib = reader.read(14, dib_size);
    const auto core = dib_size == 12;
    const auto width = integer(dib, 4, core ? 2 : 4, true);
    const auto signed_height = core ? static_cast<std::int64_t>(integer(dib, 6, 2, true)) :
        static_cast<std::int64_t>(std::bit_cast<std::int32_t>(static_cast<std::uint32_t>(integer(dib, 8, 4, true))));
    const auto height = static_cast<std::uint64_t>(signed_height < 0 ? -signed_height : signed_height);
    const auto bits = integer(dib, core ? 10 : 14, 2, true);
    const auto compression = dib_size >= 40 ? integer(dib, 16, 4, true) : 0;
    if (compression == 0 || compression == 3 || compression == 6) {
        const auto row = ((width * bits + 31) / 32) * 4;
        const auto payload = row * height;
        if (payload > extent - pixels) { throw Failure{ParseStatus::Malformed, "BMP pixel array exceeds declared file size", reader.base + pixels}; }
    }
    if (extent != reader.size) { reader.diagnostic("trailing_data", "Bytes remain after declared BMP size", extent); }
}

void inspect_ico(Reader& reader, const bool metadata) {
    const auto header = reader.read(0, 6);
    const auto kind = integer(header, 2, 2, true), count = integer(header, 4, 2, true);
    if (integer(header, 0, 2, true) != 0 || (kind != 1 && kind != 2) || count == 0) { throw Failure{ParseStatus::Malformed, "Invalid icon directory", reader.base}; }
    reader.require(6, count * 16);
    reader.number("image_count", 4, 2, true);
    for (std::uint64_t index = 0; index < count; ++index) {
        reader.entry();
        const auto at = 6 + index * 16;
        const auto entry = reader.read(at, 16);
        const auto length = integer(entry, 8, 4, true), offset = integer(entry, 12, 4, true);
        if (offset < 6 + count * 16 || length == 0) { throw Failure{ParseStatus::Malformed, "Icon image overlaps directory", reader.base + at}; }
        reader.require(offset, length);
        Reader field(reader.context, reader.base, reader.size, reader.group + ".entry" + std::to_string(index));
        field.put("width", ValueKind::Unsigned, std::to_string(entry[0] == 0 ? 256 : entry[0]), at, reader.read(at, 1));
        field.put("height", ValueKind::Unsigned, std::to_string(entry[1] == 0 ? 256 : entry[1]), at + 1, reader.read(at + 1, 1));
        if (kind == 2) { field.number("hotspot_x", at + 4, 2, true); field.number("hotspot_y", at + 6, 2, true); }
        else { field.number("planes", at + 4, 2, true); field.number("bits_per_pixel", at + 6, 2, true); }
        Reader child(reader.context, reader.base + offset, length, field.group + ".image");
        if (!reader.context.budget().check_depth(1)) { throw Failure{ParseStatus::Internal, "Embedded image depth budget exceeded", reader.base + offset}; }
        if (starts(child.read(0, std::min<std::uint64_t>(8, length)), std::string_view("\x89PNG\r\n\x1a\n", 8))) {
            inspect_png(child, metadata);
        } else {
            inspect_dib(child, 0, true);
        }
    }
}

}

namespace metaxtract::native {
namespace {

class ImageStructureParser final : public Parser {
    bool metadata_;
public:
    explicit ImageStructureParser(bool metadata = false) : metadata_(metadata) {}
    const ParserCapability& capability() const noexcept override {
        static const ParserCapability value{"images.structure", "MetaXtract image structure parser", "builtin",
            Probe | Extract, {"jpeg", "png", "tiff", "bigtiff", "webp", "gif", "bmp", "ico", "cur"}};
        static const ParserCapability detailed{"images.metadata", "MetaXtract image metadata parser", "builtin",
            Probe | Extract, {"jpeg", "png", "tiff", "bigtiff", "webp", "gif", "bmp", "ico", "cur"}};
        return metadata_ ? detailed : value;
    }

    ParseStatus run(ParserContext& context) const override {
        using namespace formats;
        Reader reader(context, 0, context.source_size(), "image");
        try {
            const auto header = reader.read(0, std::min<std::uint64_t>(16, reader.size));
            if (starts(header, std::string_view("\xff\xd8", 2))) { reader.group = "image.jpeg"; inspect_jpeg(reader, metadata_); }
            else if (starts(header, std::string_view("\x89PNG\r\n\x1a\n", 8))) { reader.group = "image.png"; inspect_png(reader, metadata_); }
            else if (starts(header, "II") || starts(header, "MM")) { reader.group = "image.tiff"; inspect_tiff(reader, metadata_); }
            else if (starts(header, "RIFF") && starts(header, "WEBP", 8)) { reader.group = "image.webp"; inspect_webp(reader, metadata_); }
            else if (starts(header, "GIF87a") || starts(header, "GIF89a")) { reader.group = "image.gif"; inspect_gif(reader, metadata_); }
            else if (starts(header, "BM")) { reader.group = "image.bmp"; inspect_bmp(reader); }
            else if (starts(header, std::string_view("\0\0\1\0", 4)) || starts(header, std::string_view("\0\0\2\0", 4))) { reader.group = "image.ico"; inspect_ico(reader, metadata_); }
            else { return ParseStatus::Unsupported; }
            return ParseStatus::Complete;
        } catch (const Failure& failure) {
            if (context.budget().status() == BudgetStatus::Ok) {
                reader.diagnostic("structural_error", failure.message, failure.offset >= reader.base ? failure.offset - reader.base : 0);
            }
            return failure.status;
        }
    }
};

}

std::unique_ptr<Parser> image_structure_parser() {
    return std::make_unique<ImageStructureParser>();
}

std::unique_ptr<Parser> image_metadata_parser() {
    return std::make_unique<ImageStructureParser>(true);
}

}
