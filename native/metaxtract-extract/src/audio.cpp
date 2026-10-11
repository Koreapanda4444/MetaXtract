#include "format.hpp"

#include <fileref.h>
#include <tiostream.h>
#include <tfile.h>
#include <tpropertymap.h>
#include <memory>
#include <bit>
#include <cmath>
#include <map>

namespace metaxtract::native::formats {
namespace {

class TagStream final : public TagLib::IOStream {
    Reader& reader_;
    TagLib::offset_t position_ = 0;
    [[noreturn]] void denied() const { throw Failure{ParseStatus::InvalidRequest, "Native tag stream is read-only", static_cast<std::uint64_t>(std::max<TagLib::offset_t>(position_, 0))}; }
public:
    explicit TagStream(Reader& reader) : reader_(reader) {}
    TagLib::FileName name() const override { return TagLib::FileName("memory"); }
    TagLib::ByteVector readBlock(std::size_t length) override {
        reader_.checkpoint();
        if (position_ < 0 || static_cast<std::uint64_t>(position_) >= reader_.size) { return {}; }
        const auto count = std::min<std::uint64_t>(length, reader_.size - static_cast<std::uint64_t>(position_));
        if (!reader_.context.budget().reserve_allocation(count)) { throw Failure{ParseStatus::Internal, "TagLib copy budget exhausted", static_cast<std::uint64_t>(position_)}; }
        const auto bytes = reader_.read(static_cast<std::uint64_t>(position_), count);
        position_ += static_cast<TagLib::offset_t>(count);
        return TagLib::ByteVector(reinterpret_cast<const char*>(bytes.data()), static_cast<unsigned>(bytes.size()));
    }
    void writeBlock(const TagLib::ByteVector&) override { denied(); }
    void insert(const TagLib::ByteVector&, TagLib::offset_t, std::size_t) override { denied(); }
    void removeBlock(TagLib::offset_t, std::size_t) override { denied(); }
    bool readOnly() const override { return true; }
    bool isOpen() const override { return true; }
    void seek(TagLib::offset_t offset, Position origin) override {
        reader_.checkpoint();
        const TagLib::offset_t base = origin == Beginning ? 0 : origin == Current ? position_ : static_cast<TagLib::offset_t>(reader_.size);
        if ((offset > 0 && base > std::numeric_limits<TagLib::offset_t>::max() - offset) ||
            (offset < 0 && base < std::numeric_limits<TagLib::offset_t>::min() - offset)) {
            throw Failure{ParseStatus::Malformed, "TagLib seek integer overflow", 0};
        }
        position_ = base + offset;
    }
    TagLib::offset_t tell() const override { return static_cast<TagLib::offset_t>(position_); }
    TagLib::offset_t length() override { return static_cast<TagLib::offset_t>(reader_.size); }
    void truncate(TagLib::offset_t) override { denied(); }
};

std::uint64_t syncsafe(Reader& reader, const std::uint64_t at) {
    const auto data = reader.read(at, 4);
    std::uint64_t result = 0;
    for (auto byte : data) {
        if (byte & 0x80U) { throw Failure{ParseStatus::Malformed, "Invalid ID3 syncsafe integer", reader.base + at}; }
        result = (result << 7U) | byte;
    }
    return result;
}

void text(Reader& reader, const std::string& key, const std::uint64_t at, const std::uint64_t length, const std::string& encoding = "UTF-8") {
    auto bytes = reader.read(at, length);
    auto count = bytes.size();
    while (count && bytes[count - 1] == 0) { --count; }
    std::string value(reinterpret_cast<const char*>(bytes.data()), count);
    reader.put(key, ValueKind::Text, std::move(value), at, std::move(bytes), encoding);
}

std::uint64_t id3(Reader& source, const std::uint64_t start) {
    auto reader = source.window(start, source.size - start, source.group + ".id3");
    const auto header = reader.read(0, 10);
    if (!starts(header, "ID3")) { return 0; }
    const auto version = header[3];
    if (version < 2 || version > 4 || header[4] == 255) { throw Failure{ParseStatus::Unsupported, "Unsupported ID3 version", reader.base + 3}; }
    const auto length = syncsafe(reader, 6);
    reader.require(10, length);
    reader.number("version", 3, 1);
    reader.number("flags", 5, 1);
    reader.put("raw_tag", ValueKind::Bytes, {}, 0, reader.read(0, length + 10));
    if ((header[5] & 0x80U) || (version == 2 && (header[5] & 0x40U))) {
        reader.diagnostic("encoded_tag", "Whole-tag unsynchronization or compression retained for TagLib decoding", 5);
        return length + 10;
    }
    std::uint64_t at = 10;
    const auto end = length + 10;
    if (version >= 3 && (header[5] & 0x40U)) {
        const auto extended = version == 4 ? syncsafe(reader, at) : integer(reader.read(at, 4), 0, 4) + 4;
        if (extended < 6 || extended > end - at) { throw Failure{ParseStatus::Malformed, "ID3 extended header escapes tag", reader.base + at}; }
        at += extended;
    }
    unsigned index = 0;
    while (at < end) {
        reader.entry();
        const auto frame_header = version == 2 ? 6U : 10U;
        if (reader.read(at, 1)[0] == 0) { break; }
        if (end - at < frame_header) { throw Failure{ParseStatus::Malformed, "Truncated ID3 frame", reader.base + at}; }
        const auto h = reader.read(at, frame_header);
        const auto id_length = version == 2 ? 3U : 4U;
        for (unsigned n = 0; n < id_length; ++n) {
            if (!((h[n] >= 'A' && h[n] <= 'Z') || (h[n] >= '0' && h[n] <= '9'))) { throw Failure{ParseStatus::Malformed, "Invalid ID3 frame identifier", reader.base + at}; }
        }
        const std::string name(reinterpret_cast<const char*>(h.data()), id_length);
        const auto size = version == 4 ? syncsafe(reader, at + 4) : integer(h, id_length, version == 2 ? 3 : 4);
        if (size > end - at - frame_header) { throw Failure{ParseStatus::Malformed, "ID3 frame escapes tag", reader.base + at}; }
        auto frame = reader.window(at + frame_header, size, reader.group + ".frame" + std::to_string(index++));
        frame.decoded("frame_id", ValueKind::Text, name, "id3.frame", 0, "ASCII");
        if (version >= 3) { reader.number("frame_flags", at + 8, 2); }
        frame.put("payload", ValueKind::Bytes, {}, 0, frame.read(0, size));
        const auto flags = version == 2 ? 0 : integer(h, 8, 2);
        if (name.front() == 'T' && size > 1 && flags == 0) {
            const auto encoding = frame.read(0, 1)[0];
            if (encoding == 0 || encoding == 3) { text(frame, "text", 1, size - 1, encoding == 0 ? "ISO-8859-1" : "UTF-8"); }
        }
        at += frame_header + size;
    }
    const auto footer = version == 4 && (header[5] & 0x10U) ? 10U : 0U;
    reader.require(end, footer);
    if (footer && !starts(reader.read(end, 3), "3DI")) { throw Failure{ParseStatus::Malformed, "ID3 footer missing", reader.base + end}; }
    return end + footer;
}

void comments(Reader& reader, std::uint64_t at) {
    const auto vendor_length = integer(reader.read(at, 4), 0, 4, true); at += 4;
    text(reader, "vendor", at, vendor_length); at += vendor_length;
    const auto count = integer(reader.read(at, 4), 0, 4, true); at += 4;
    for (std::uint64_t n = 0; n < count; ++n) {
        reader.entry();
        const auto length = integer(reader.read(at, 4), 0, 4, true); at += 4;
        text(reader, "comment", at, length); at += length;
    }
}

void flac(Reader& reader, const std::uint64_t start) {
    std::uint64_t at = start + 4;
    bool last = false;
    unsigned index = 0;
    while (!last) {
        reader.entry();
        const auto header = reader.read(at, 4);
        last = (header[0] & 128U) != 0;
        const auto type = header[0] & 127U;
        const auto length = integer(header, 1, 3);
        auto block = reader.window(at + 4, length, "audio.flac.block" + std::to_string(index++));
        reader.put("block_type", ValueKind::Unsigned, std::to_string(type), at, reader.read(at, 1));
        if (type != 1) { block.put("payload", ValueKind::Bytes, {}, 0, block.read(0, length)); }
        if (type == 0) {
            if (length != 34) { throw Failure{ParseStatus::Malformed, "FLAC STREAMINFO size is not 34", block.base}; }
            block.number("min_block_size", 0, 2); block.number("max_block_size", 2, 2);
            const auto info = integer(block.read(10, 8), 0, 8);
            block.derived("sample_rate", info >> 44); block.derived("channels", ((info >> 41) & 7U) + 1);
            block.derived("bits_per_sample", ((info >> 36) & 31U) + 1);
            block.derived("total_samples", info & 0xfffffffffULL);
            block.put("pcm_md5", ValueKind::Bytes, {}, 18, block.read(18, 16));
        } else if (type == 4) { comments(block, 0); }
        at += 4 + length;
    }
    reader.derived("audio_offset", at);
}

void riff(Reader& reader, const bool aiff, const bool little) {
    const auto length = integer(reader.read(4, 4), 0, 4, little);
    if (length < 4) { throw Failure{ParseStatus::Malformed, "Audio RIFF/FORM size is too small", reader.base + 4}; }
    reader.require(8, length);
    auto end = length + 8;
    unsigned index = 0;
    for (std::uint64_t at = 12; at < end;) {
        reader.entry();
        if (end - at < 8) { throw Failure{ParseStatus::Malformed, "Truncated audio chunk", reader.base + at}; }
        const auto h = reader.read(at, 8);
        const auto size = integer(h, 4, 4, little);
        if (size > end - at - 8) { throw Failure{ParseStatus::Malformed, "Audio chunk escapes container", reader.base + at}; }
        const std::string name(reinterpret_cast<const char*>(h.data()), 4);
        auto chunk = reader.window(at + 8, size, std::string(aiff ? "audio.aiff.chunk" : "audio.wav.chunk") + std::to_string(index++));
        chunk.decoded("chunk_id", ValueKind::Text, name, "riff.chunk", 0, "ASCII");
        if (name == "data" || name == "SSND") { chunk.derived("audio_bytes", size); }
        else {
            chunk.put("payload", ValueKind::Bytes, {}, 0, chunk.read(0, size));
            if (name == "fmt ") {
                chunk.require(0, 16);
                chunk.number("format_code", 0, 2, little); chunk.number("channels", 2, 2, little);
                chunk.number("sample_rate", 4, 4, little); chunk.number("byte_rate", 8, 4, little);
                chunk.number("block_align", 12, 2, little); chunk.number("bits_per_sample", 14, 2, little);
                if (size >= 40 && integer(chunk.read(0, 2), 0, 2, little) == 0xfffe) {
                    chunk.number("valid_bits_per_sample", 18, 2, little); chunk.number("channel_mask", 20, 4, little);
                    chunk.put("subformat_guid", ValueKind::Bytes, {}, 24, chunk.read(24, 16));
                }
            } else if (name == "COMM") {
                chunk.require(0, 18); chunk.number("channels", 0, 2); chunk.number("sample_frames", 2, 4);
                chunk.number("bits_per_sample", 6, 2);
                const auto rate = chunk.read(8, 10);
                const auto exponent = integer(rate, 0, 2) & 0x7fffU;
                const auto mantissa = integer(rate, 2, 8);
                if (exponent != 0 && exponent != 0x7fffU) {
                    const auto value = std::ldexp(static_cast<double>(mantissa), static_cast<int>(exponent) - 16383 - 63);
                    chunk.put("sample_rate", ValueKind::Float, std::to_string((rate[0] & 128U) ? -value : value), 8, rate);
                }
                if (size >= 22) { text(chunk, "compression", 18, 4, "ASCII"); }
            } else if (name == "LIST" && size >= 4 && starts(chunk.read(0, 4), "INFO")) {
                for (std::uint64_t pos = 4; pos < size;) {
                    chunk.entry(); const auto item = chunk.read(pos, 8); const auto n = integer(item, 4, 4, little);
                    const std::string key(reinterpret_cast<const char*>(item.data()), 4);
                    text(chunk, key, pos + 8, n, "unspecified"); pos += 8 + n + (n & 1U);
                    if (pos > size) { throw Failure{ParseStatus::Malformed, "INFO chunk padding escapes LIST", chunk.base + pos}; }
                }
            } else if (name == "bext") {
                chunk.require(0, 602); text(chunk, "description", 0, 256, "ASCII"); text(chunk, "originator", 256, 32, "ASCII");
                text(chunk, "originator_reference", 288, 32, "ASCII"); text(chunk, "origination_date", 320, 10, "ASCII");
                text(chunk, "origination_time", 330, 8, "ASCII"); chunk.number("time_reference", 338, 8, true);
                chunk.number("bext_version", 346, 2, true);
            } else if (name == "NAME" || name == "AUTH" || name == "ANNO" || name == "(c) ") { text(chunk, "text", 0, size, "unspecified"); }
            else if (name == "ID3 " || name == "id3 ") { static_cast<void>(id3(chunk, 0)); }
            else if (name == "iXML" || name == "axml") { inspect_xmp(chunk); }
        }
        at += 8 + size + (size & 1U);
        if (at > end) { throw Failure{ParseStatus::Malformed, "Audio chunk padding escapes container", reader.base + at}; }
    }
    if (end < reader.size) { reader.diagnostic("trailing_data", "Data remains after audio RIFF/FORM extent", end); }
}

void ogg(Reader& reader) {
    struct Stream { std::vector<std::uint8_t> packet; unsigned packets = 0; bool continued = false; std::uint32_t sequence = 0; bool seen = false; };
    std::map<std::uint32_t, Stream> streams;
    for (std::uint64_t at = 0; at < reader.size;) {
        reader.entry(); const auto header = reader.read(at, 27);
        if (!starts(header, "OggS") || header[4] != 0) { throw Failure{ParseStatus::Malformed, "Invalid Ogg page header", reader.base + at}; }
        const auto serial = static_cast<std::uint32_t>(integer(header, 14, 4, true));
        const auto sequence = static_cast<std::uint32_t>(integer(header, 18, 4, true));
        const auto lacing = reader.read(at + 27, header[26]);
        std::uint64_t data_length = 0; for (auto n : lacing) { data_length += n; }
        auto page = reader.window(at, 27 + lacing.size() + data_length, "audio.ogg.stream" + std::to_string(serial));
        page.number("serial", 14, 4, true); page.number("sequence", 18, 4, true); page.number("granule_position", 6, 8, true);
        page.number("page_flags", 5, 1);
        auto& state = streams[serial];
        if (state.seen && sequence != state.sequence + 1U) { page.diagnostic("sequence_gap", "Ogg page sequence is discontinuous", 18); }
        if (((header[5] & 1U) != 0) != state.continued) { throw Failure{ParseStatus::Malformed, "Ogg packet continuation is inconsistent", page.base + 5}; }
        state.seen = true; state.sequence = sequence;
        std::uint64_t pos = 27 + lacing.size();
        for (const auto n : lacing) {
            page.entry();
            if (state.packets < 2) {
                auto bytes = page.read(pos, n);
                if (!reader.context.budget().reserve_allocation(n) || !reader.context.budget().consume_decoded(n)) { throw Failure{ParseStatus::Internal, "Ogg header assembly budget exceeded", page.base + pos}; }
                state.packet.insert(state.packet.end(), bytes.begin(), bytes.end());
            }
            pos += n; state.continued = n == 255;
            if (n < 255) {
                if (state.packets < 2) {
                    Reader packet(reader.context, state.packet, page.group + ".packet" + std::to_string(state.packets), page.group + ".packet" + std::to_string(state.packets));
                    packet.put("packet", ValueKind::Bytes, {}, 0, packet.read(0, packet.size));
                    if (starts(state.packet, "OpusHead")) {
                        packet.require(0, 19); packet.number("opus_version", 8, 1); packet.number("channels", 9, 1);
                        packet.number("pre_skip", 10, 2, true); packet.number("input_sample_rate", 12, 4, true);
                        packet.derived("sample_rate", 48000); packet.number("mapping_family", 18, 1);
                    } else if (starts(state.packet, "OpusTags")) { comments(packet, 8); }
                    else if (starts(state.packet, "\1vorbis")) {
                        packet.require(0, 30); packet.number("channels", 11, 1); packet.number("sample_rate", 12, 4, true);
                        packet.number("bitrate_nominal", 20, 4, true);
                    } else if (starts(state.packet, "\3vorbis")) { comments(packet, 7); }
                    state.packet.clear();
                }
                ++state.packets;
            }
        }
        at += page.size;
    }
    for (const auto& [serial, stream] : streams) {
        static_cast<void>(serial);
        if (stream.continued) { throw Failure{ParseStatus::Malformed, "Ogg ends in an incomplete packet", reader.base + reader.size}; }
    }
}

void trailing_tags(Reader& reader) {
    auto end = reader.size;
    if (end >= 128 && starts(reader.read(end - 128, 3), "TAG")) {
        auto tag = reader.window(end - 128, 128, "audio.id3v1");
        text(tag, "title", 3, 30, "ISO-8859-1"); text(tag, "artist", 33, 30, "ISO-8859-1"); text(tag, "album", 63, 30, "ISO-8859-1");
        text(tag, "year", 93, 4, "ASCII"); text(tag, "comment", 97, 30, "ISO-8859-1"); tag.number("genre", 127, 1);
        const auto track = tag.read(125, 2); if (track[0] == 0 && track[1] != 0) { tag.number("track", 126, 1); }
        end -= 128;
    }
    if (end < 32 || !starts(reader.read(end - 32, 8), "APETAGEX")) { return; }
    const auto footer = reader.read(end - 32, 32);
    const auto size = integer(footer, 12, 4, true), count = integer(footer, 16, 4, true);
    if (size < 32 || size > end) { throw Failure{ParseStatus::Malformed, "APE tag size escapes source", reader.base + end - 20}; }
    auto tag = reader.window(end - size, size, "audio.apev2");
    std::uint64_t at = 0;
    for (std::uint64_t n = 0; n < count; ++n) {
        tag.entry(); const auto h = tag.read(at, 8); const auto length = integer(h, 0, 4, true), flags = integer(h, 4, 4, true); at += 8;
        std::string key;
        for (unsigned p = 0; p < 256; ++p) { const auto byte = tag.read(at++, 1)[0]; if (byte == 0) { break; } key.push_back(static_cast<char>(byte)); }
        if (key.empty() || at > size - 32 || length > size - 32 - at) { throw Failure{ParseStatus::Malformed, "APE item escapes tag", tag.base + at}; }
        auto item = tag.window(at, length, tag.group + ".item" + std::to_string(n));
        item.decoded("key", ValueKind::Text, key, "ape.item", 0, "ASCII"); item.derived("flags", flags);
        if (((flags >> 1U) & 3U) == 0) { text(item, "text", 0, length); }
        else { item.put("payload", ValueKind::Bytes, {}, 0, item.read(0, length)); }
        at += length;
    }
    if (at != size - 32) { tag.diagnostic("unused_tag_bytes", "APE tag has unclaimed item bytes", at); }
}

void technical_header(Reader& reader, const std::span<const std::uint8_t> header, const std::uint64_t start) {
    if (starts(header, "wvpk")) {
        reader.require(start, 32); reader.number("block_size", start + 4, 4, true); reader.number("format_version", start + 8, 2, true);
        reader.number("total_samples", start + 12, 4, true); reader.number("block_samples", start + 20, 4, true); reader.number("flags", start + 24, 4, true);
        const auto size = integer(header, 4, 4, true); if (size < 24) { throw Failure{ParseStatus::Malformed, "WavPack block is shorter than header", reader.base + start}; }
        reader.require(start + 8, size);
    } else if (starts(header, "MAC ")) {
        reader.number("format_version", start + 4, 2, true);
        if (integer(header, 4, 2, true) >= 3980) {
            reader.require(start, 52); const auto descriptor = integer(reader.read(start + 8, 4), 0, 4, true);
            if (descriptor < 52) { throw Failure{ParseStatus::Malformed, "APE descriptor too short", reader.base + start + 8}; }
            auto h = reader.window(start + descriptor, 24, "audio.ape.header");
            h.number("compression_level", 0, 2, true); h.number("format_flags", 2, 2, true); h.number("blocks_per_frame", 4, 4, true);
            h.number("final_frame_blocks", 8, 4, true); h.number("total_frames", 12, 4, true); h.number("bits_per_sample", 16, 2, true);
            h.number("channels", 18, 2, true); h.number("sample_rate", 20, 4, true);
        }
    } else if (header.size() >= 4 && header[0] == 255 && (header[1] & 0xe0U) == 0xe0U) {
        const auto version = (header[1] >> 3U) & 3U, layer = (header[1] >> 1U) & 3U, rate_index = (header[2] >> 2U) & 3U;
        if (version == 1 || layer == 0 || rate_index == 3 || (header[2] >> 4U) == 15) { throw Failure{ParseStatus::Malformed, "Invalid MPEG audio header", reader.base + start}; }
        constexpr unsigned rates[] = {44100, 48000, 32000};
        reader.derived("sample_rate", rates[rate_index] / (version == 3 ? 1 : version == 2 ? 2 : 4));
        reader.derived("channels", (header[3] >> 6U) == 3 ? 1 : 2); reader.derived("mpeg_version_code", version); reader.derived("layer", 4 - layer);
    }
}

class AudioParser final : public Parser {
public:
    const ParserCapability& capability() const noexcept override {
        static const ParserCapability value{"audio.metadata", "Bounded audio tags and TagLib", "1", Extract, {"mp3", "flac", "ogg", "opus", "wav", "aiff", "ape", "wavpack", "m4a", "asf", "dsf"}};
        return value;
    }
    ParseStatus run(ParserContext& context) const override {
        if (context.source_size() < 4 || context.source_size() > static_cast<std::uint64_t>(std::numeric_limits<TagLib::offset_t>::max())) { return ParseStatus::Unsupported; }
        Reader reader(context, 0, context.source_size(), "audio.structure");
        try {
            auto header = reader.read(0, std::min<std::uint64_t>(32, reader.size));
            std::uint64_t start = 0;
            if (starts(header, "ID3")) { start = id3(reader, 0); header = reader.read(start, std::min<std::uint64_t>(32, reader.size - start)); }
            if (starts(header, "fLaC")) { flac(reader, start); }
            else if (starts(header, "OggS")) { ogg(reader); }
            else if (starts(header, "RIFF") || starts(header, "RIFX")) {
                if (header.size() < 12 || !starts(header, "WAVE", 8)) { return ParseStatus::Unsupported; }
                riff(reader, false, starts(header, "RIFF"));
            } else if (starts(header, "FORM")) {
                if (header.size() < 12 || (!starts(header, "AIFF", 8) && !starts(header, "AIFC", 8))) { return ParseStatus::Unsupported; }
                riff(reader, true, false);
            } else { technical_header(reader, header, start); }
            trailing_tags(reader);
            if (!context.budget().reserve_allocation(16 * 1024 * 1024)) { return ParseStatus::Internal; }
            TagStream stream(reader);
            TagLib::FileRef ref(&stream, true, TagLib::AudioProperties::Average);
            if (ref.isNull() || !ref.file() || !ref.file()->isValid()) {
                reader.diagnostic("taglib_unavailable", "TagLib could not fully validate this audio file; original observations retained", 0);
                return ParseStatus::Unsupported;
            }
            reader.group = "audio.taglib";
            reader.decoded("library_version", ValueKind::Text, std::to_string(TAGLIB_MAJOR_VERSION) + "." + std::to_string(TAGLIB_MINOR_VERSION) + "." + std::to_string(TAGLIB_PATCH_VERSION), "taglib", 2);
            const auto properties = ref.properties();
            for (auto it = properties.begin(); it != properties.end(); ++it) {
                reader.entry();
                for (const auto& value : it->second) { reader.decoded(it->first.to8Bit(true), ValueKind::Text, value.to8Bit(true), "taglib", 2); }
            }
            for (const auto& key : properties.unsupportedData()) { reader.decoded("unsupported_property", ValueKind::Text, key.to8Bit(true), "taglib", 2); }
            if (const auto* p = ref.audioProperties()) {
                for (const auto& [key, value] : std::initializer_list<std::pair<const char*, int>>{{"duration_ms", p->lengthInMilliseconds()}, {"sample_rate", p->sampleRate()}, {"channels", p->channels()}, {"bitrate_kbps", p->bitrate()}}) {
                    if (value >= 0) { reader.decoded(key, ValueKind::Unsigned, std::to_string(value), "taglib", 2); }
                }
            }
            return ParseStatus::Complete;
        } catch (const Failure& failure) { reader.diagnostic("malformed", failure.message, std::min(reader.size, failure.offset)); return failure.status; }
    }
};
}
}

namespace metaxtract::native {
std::unique_ptr<Parser> audio_metadata_parser() { return std::make_unique<formats::AudioParser>(); }
}
