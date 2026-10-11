#include "metaxtract/native/detection.hpp"

#include <algorithm>
#include <array>
#include <cstring>
#include <string_view>

namespace metaxtract::native {
namespace {

std::uint32_t number(const std::vector<std::uint8_t>& bytes, const std::size_t at, const unsigned width, const bool little = false) {
    std::uint32_t result = 0;
    for (unsigned index = 0; index < width; ++index) {
        const auto shift = little ? index : width - index - 1;
        result |= static_cast<std::uint32_t>(bytes.at(at + index)) << (8U * shift);
    }
    return result;
}

bool match(const std::vector<std::uint8_t>& bytes, const std::size_t at, const std::string_view magic) {
    return at <= bytes.size() && magic.size() <= bytes.size() - at &&
        std::memcmp(bytes.data() + at, magic.data(), magic.size()) == 0;
}

}

std::vector<ContentCandidate> detect_content(ParserContext& context) {
    auto chunk = context.read_at(0, static_cast<std::uint32_t>(std::min<std::uint64_t>(context.source_size(), 65536)));
    if (chunk.status != ReadStatus::Ok) {
        const auto budget = static_cast<std::uint32_t>(context.budget().status());
        const auto status = budget != 0 ? budget :
            chunk.status == ReadStatus::OutOfRange ? 5U :
            chunk.status == ReadStatus::RequestTooLarge ? 2U :
            chunk.status == ReadStatus::BudgetExhausted ? 11U :
            chunk.status == ReadStatus::IoFailure ? 6U :
            chunk.status == ReadStatus::AllocationFailure ? 8U : 3U;
        throw DetectionFailure{status};
    }
    const auto& bytes = chunk.bytes;
    std::vector<ContentCandidate> result;
    const auto add = [&](const char* format, const char* mime, const char* container, const std::uint64_t offset,
                         const std::uint64_t length, const DetectionState state, const char* detail) {
        const auto charge = 4 * (sizeof(ContentCandidate) + std::strlen(format) + std::strlen(mime) + std::strlen(container) + std::strlen(detail));
        if (context.budget().add_observations(1) && context.budget().reserve_allocation(charge)) {
            result.push_back({format, mime, container, state, offset, length, detail});
        }
    };
    const auto header = [&](const std::size_t minimum, const bool valid) {
        return bytes.size() < minimum ? DetectionState::Truncated : valid ? DetectionState::HeaderValidated : DetectionState::Invalid;
    };
    if (match(bytes, 0, std::string_view("\xff\xd8\xff", 3))) {
        add("jpeg", "image/jpeg", "jpeg", 0, 3, DetectionState::Signature, "SOI and marker prefix; image structure not yet validated");
    }
    if (match(bytes, 0, std::string_view("\x89PNG\r\n\x1a\n", 8))) {
        const auto valid = bytes.size() >= 33 && number(bytes, 8, 4) == 13 && match(bytes, 12, "IHDR") &&
            number(bytes, 16, 4) != 0 && number(bytes, 20, 4) != 0;
        add("png", "image/png", "png", 0, std::min<std::size_t>(bytes.size(), 33), header(33, valid), "PNG signature and IHDR size/dimensions; chunk CRC and image data require extraction");
    }
    if (match(bytes, 0, "II") || match(bytes, 0, "MM")) {
        const bool little = match(bytes, 0, "II");
        if (bytes.size() >= 4) {
            const auto version = number(bytes, 2, 2, little);
            if (version == 42 || version == 43) {
                const auto minimum = version == 43 ? 16U : 8U;
                const auto valid = bytes.size() >= minimum && (version != 43 || (number(bytes, 4, 2, little) == 8 && number(bytes, 6, 2, little) == 0));
                const auto cr2 = match(bytes, 8, std::string_view("CR\2\0", 4));
                add(cr2 ? "cr2" : version == 43 ? "bigtiff" : "tiff", cr2 ? "image/x-canon-cr2" : "image/tiff", "tiff", 0,
                    std::min<std::size_t>(minimum, bytes.size()), header(minimum, valid), "TIFF header; directory offsets and tags require extraction");
            }
        }
    }
    if (match(bytes, 0, "GIF87a") || match(bytes, 0, "GIF89a")) {
        const auto valid = bytes.size() >= 13 && number(bytes, 6, 2, true) != 0 && number(bytes, 8, 2, true) != 0;
        add("gif", "image/gif", "gif", 0, std::min<std::size_t>(13, bytes.size()), header(13, valid), "GIF logical screen header; blocks require extraction");
    }
    if (match(bytes, 0, "BM")) {
        const auto valid = bytes.size() >= 26 && number(bytes, 10, 4, true) >= 14 && number(bytes, 14, 4, true) >= 12;
        add("bmp", "image/bmp", "bmp", 0, std::min<std::size_t>(26, bytes.size()), header(26, valid), "BMP file and DIB header; pixel range requires extraction");
    }
    if (match(bytes, 0, std::string_view("\0\0\1\0", 4)) || match(bytes, 0, std::string_view("\0\0\2\0", 4))) {
        const auto valid = bytes.size() >= 6 && number(bytes, 4, 2, true) > 0;
        add(bytes[2] == 1 ? "ico" : "cur", "image/x-icon", "ico", 0, std::min<std::size_t>(6, bytes.size()), header(6, valid), "Icon directory header; image entries require extraction");
    }
    if (match(bytes, 0, "RIFF") || match(bytes, 0, "RF64")) {
        if (bytes.size() >= 12) {
            const char* format = match(bytes, 8, "WEBP") ? "webp" : match(bytes, 8, "WAVE") ? "wav" : match(bytes, 8, "AVI ") ? "avi" : "riff";
            const char* mime = std::strcmp(format, "webp") == 0 ? "image/webp" : std::strcmp(format, "wav") == 0 ? "audio/wav" : std::strcmp(format, "avi") == 0 ? "video/x-msvideo" : "application/octet-stream";
            const auto claimed = static_cast<std::uint64_t>(number(bytes, 4, 4, true)) + 8;
            const auto state = match(bytes, 0, "RF64") ? DetectionState::Signature : claimed > context.source_size() ? DetectionState::Truncated :
                claimed >= 12 ? DetectionState::HeaderValidated : DetectionState::Invalid;
            add(format, mime, "riff", 0, 12, state, "RIFF form and declared size; RF64 uses a separate ds64 size");
        } else {
            add("riff", "application/octet-stream", "riff", 0, bytes.size(), DetectionState::Truncated, "Incomplete RIFF form header");
        }
    }
    if (match(bytes, 0, "FORM")) {
        const auto valid = bytes.size() >= 12 && (match(bytes, 8, "AIFF") || match(bytes, 8, "AIFC"));
        if (bytes.size() < 12 || valid) {
            const auto state = bytes.size() < 12 ? DetectionState::Truncated :
                static_cast<std::uint64_t>(number(bytes, 4, 4)) + 8 > context.source_size() ? DetectionState::Truncated : DetectionState::HeaderValidated;
            add("aiff", "audio/aiff", "iff", 0, std::min<std::size_t>(12, bytes.size()), state, "IFF audio form; chunks require extraction");
        }
    }
    if (match(bytes, 0, "fLaC")) {
        add("flac", "audio/flac", "flac", 0, std::min<std::size_t>(8, bytes.size()), header(8, bytes.size() >= 8 && (bytes[4] & 0x7fU) == 0 && number(bytes, 5, 3) == 34), "FLAC signature and STREAMINFO header");
    }
    if (match(bytes, 0, "OggS")) {
        add("ogg", "audio/ogg", "ogg", 0, std::min<std::size_t>(27, bytes.size()), header(27, bytes.size() >= 27 && bytes[4] == 0), "Ogg capture and page version; packets and checksums require extraction");
    }
    if (match(bytes, 0, "ID3")) {
        bool valid = bytes.size() >= 10 && bytes[3] >= 2 && bytes[3] <= 4 && bytes[4] != 255;
        if (bytes.size() >= 10) {
            for (std::size_t index = 6; index < 10; ++index) { valid &= (bytes[index] & 0x80U) == 0; }
        }
        add("mp3", "audio/mpeg", "id3", 0, std::min<std::size_t>(10, bytes.size()), header(10, valid), "ID3 header; audio frame type is not established by ID3 alone");
    } else if (bytes.size() >= 4 && bytes[0] == 255 && (bytes[1] & 0xe0U) == 0xe0U &&
               ((bytes[1] >> 3U) & 3U) != 1 && ((bytes[1] >> 1U) & 3U) != 0 && (bytes[2] >> 4U) != 0 && (bytes[2] >> 4U) != 15 && ((bytes[2] >> 2U) & 3U) != 3) {
        add("mpeg_audio", "audio/mpeg", "mpeg_audio", 0, 4, DetectionState::Signature, "MPEG audio frame header; consecutive frames require extraction");
    }
    if (match(bytes, 0, "MAC ")) { add("ape", "audio/x-ape", "ape", 0, 4, DetectionState::Signature, "Monkey's Audio signature"); }
    if (match(bytes, 0, "wvpk")) { add("wavpack", "audio/wavpack", "wavpack", 0, std::min<std::size_t>(32, bytes.size()), header(32, bytes.size() >= 32 && number(bytes, 4, 4, true) >= 24), "WavPack block header"); }
    if (match(bytes, 0, std::string_view("\x1a\x45\xdf\xa3", 4))) { add("ebml", "video/x-matroska", "ebml", 0, 4, DetectionState::Signature, "EBML header; DocType distinguishes Matroska and WebM"); }
    if (match(bytes, 0, std::string_view("\0\0\1\xba", 4))) { add("mpeg_ps", "video/mpeg", "mpeg_ps", 0, 4, DetectionState::Signature, "MPEG program stream pack start"); }
    for (const std::size_t spacing : {188U, 192U, 204U}) {
        const std::size_t offset = spacing == 192 ? 4 : 0;
        if (bytes.size() > offset + spacing * 2 && bytes[offset] == 0x47 && bytes[offset + spacing] == 0x47 && bytes[offset + spacing * 2] == 0x47) {
            add("mpeg_ts", "video/mp2t", "mpeg_ts", offset, spacing * 2 + 1, DetectionState::HeaderValidated, "Three transport-stream sync bytes; packet and table validation requires extraction");
            break;
        }
    }
    if (bytes.size() >= 8 && match(bytes, 4, "ftyp")) {
        const auto size = number(bytes, 0, 4);
        if (size >= 16 && bytes.size() >= 16) {
            const auto available = std::min<std::size_t>(size, bytes.size());
            bool avif = false, heic = false, heif = false, quicktime = false;
            for (std::size_t at = 8; at + 4 <= available; at += at == 8 ? 8 : 4) {
                avif |= match(bytes, at, "avif") || match(bytes, at, "avis");
                heic |= match(bytes, at, "heic") || match(bytes, at, "heix") || match(bytes, at, "hevc") || match(bytes, at, "hevx");
                heif |= match(bytes, at, "mif1") || match(bytes, at, "msf1");
                quicktime |= match(bytes, at, "qt  ");
            }
            const auto state = size > context.source_size() ? DetectionState::Truncated : ((size - 16) % 4) == 0 ? DetectionState::HeaderValidated : DetectionState::Invalid;
            add(avif ? "avif" : heic ? "heic" : heif ? "heif" : quicktime ? "mov" : "mp4",
                avif ? "image/avif" : heic ? "image/heic" : heif ? "image/heif" : quicktime ? "video/quicktime" : "video/mp4",
                "isobmff", 0, available, state, "ISO BMFF brands and ftyp length; item/track structure requires extraction");
        } else {
            add("isobmff", "application/octet-stream", "isobmff", 0, std::min<std::size_t>(16, bytes.size()), header(16, false), "Invalid or incomplete ftyp box");
        }
    } else if (bytes.size() >= 8 && (match(bytes, 4, "moov") || match(bytes, 4, "mdat") || match(bytes, 4, "wide"))) {
        add("mov", "video/quicktime", "isobmff", 0, 8, DetectionState::Signature, "Legacy QuickTime box without ftyp");
    }
    for (std::size_t at = 0; at < std::min<std::size_t>(bytes.size(), 1024); ++at) {
        if (match(bytes, at, "%PDF-")) {
            const auto valid = bytes.size() >= at + 8 && (bytes[at + 5] == '1' || bytes[at + 5] == '2') && bytes[at + 6] == '.' && bytes[at + 7] >= '0' && bytes[at + 7] <= '9';
            add("pdf", "application/pdf", "pdf", at, std::min<std::size_t>(8, bytes.size() - at), header(at + 8, valid), "PDF header; objects and cross-reference structure require extraction");
            break;
        }
    }
    if (match(bytes, 0, std::string_view("PK\3\4", 4)) || match(bytes, 0, std::string_view("PK\5\6", 4)) || match(bytes, 0, std::string_view("PK\7\10", 4))) {
        add("zip", "application/zip", "zip", 0, 4, DetectionState::Signature, "ZIP signature; package subtype and central directory require extraction");
    }
    if (match(bytes, 0, std::string_view("\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1", 8))) {
        add("ole", "application/x-ole-storage", "ole", 0, 8, DetectionState::Signature, "Compound file signature; document subtype requires extraction");
    }
    static_cast<void>(context.budget().checkpoint());
    return result;
}

}
