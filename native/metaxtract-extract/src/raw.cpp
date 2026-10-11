#include "format.hpp"

#include <libraw/libraw.h>

#include <cstdio>
#include <climits>
#include <cstdlib>
#include <memory>

namespace metaxtract::native {
namespace {

class RawStream final : public LibRaw_abstract_datastream {
    formats::Reader& reader_;
    std::uint64_t position_ = 0;
    std::uint64_t cache_at_ = 0;
    std::vector<std::uint8_t> cache_;
public:
    explicit RawStream(formats::Reader& reader) : reader_(reader) {}
    int valid() override { return 1; }
    INT64 size() override { return static_cast<INT64>(reader_.size); }
    INT64 tell() override { return static_cast<INT64>(position_); }
    int eof() override { return position_ >= reader_.size || !reader_.context.budget().checkpoint(); }
    int seek(INT64 offset, int origin) override {
        const auto base = origin == SEEK_SET ? 0 : origin == SEEK_CUR ? static_cast<INT64>(position_) : origin == SEEK_END ? size() : -1;
        if (base < 0 || (offset < 0 && offset < -base) || (offset > 0 && offset > std::numeric_limits<INT64>::max() - base)) { return -1; }
        const auto next = base + offset;
        if (next < 0 || static_cast<std::uint64_t>(next) > reader_.size) { return -1; }
        position_ = static_cast<std::uint64_t>(next); return 0;
    }
    int read(void* destination, std::size_t width, std::size_t count) override {
        if (width == 0 || count == 0 || count > std::numeric_limits<std::size_t>::max() / width || count > INT_MAX) { return 0; }
        const auto length = std::min<std::uint64_t>(width * count, reader_.size - position_);
        std::uint64_t copied = 0;
        try {
            while (copied < length) {
                reader_.checkpoint();
                if (position_ < cache_at_ || position_ - cache_at_ >= cache_.size()) {
                    cache_at_ = position_; cache_ = reader_.read(position_, std::min<std::uint64_t>(65536, reader_.size - position_));
                }
                const auto available = std::min<std::uint64_t>(length - copied, cache_.size() - (position_ - cache_at_));
                std::memcpy(static_cast<std::uint8_t*>(destination) + copied, cache_.data() + (position_ - cache_at_), static_cast<std::size_t>(available));
                copied += available; position_ += available;
            }
        } catch (const formats::Failure&) { return static_cast<int>(copied / width); }
        return static_cast<int>(copied / width);
    }
    int get_char() override { std::uint8_t byte = 0; return read(&byte, 1, 1) == 1 ? byte : EOF; }
    char* gets(char* destination, int maximum) override {
        if (maximum <= 0) { return nullptr; }
        int index = 0;
        while (index < maximum - 1) { const auto byte = get_char(); if (byte == EOF) { break; } destination[index++] = static_cast<char>(byte); if (byte == '\n') { break; } }
        destination[index] = 0; return index == 0 ? nullptr : destination;
    }
    int scanf_one(const char* format, void* destination) override {
        char line[128]{};
        int index = 0;
        while (index < 127) { const auto byte = get_char(); if (byte == EOF || byte == '\n' || byte == ' ' || byte == '\t') { if (index != 0 || byte == EOF) { break; } continue; } line[index++] = static_cast<char>(byte); }
        char* end = nullptr;
        if (std::strcmp(format, "%d") == 0) {
            const auto value = std::strtol(line, &end, 10);
            if (end == line || value < INT_MIN || value > INT_MAX) { return 0; }
            *static_cast<int*>(destination) = static_cast<int>(value); return 1;
        }
        if (std::strcmp(format, "%f") == 0) { const auto value = std::strtof(line, &end); if (end == line) { return 0; } *static_cast<float*>(destination) = value; return 1; }
        if (std::strcmp(format, "%lf") == 0) { const auto value = std::strtod(line, &end); if (end == line) { return 0; } *static_cast<double*>(destination) = value; return 1; }
        return 0;
    }
};

int progress(void* owner, LibRaw_progress, int, int) {
    return static_cast<ParserContext*>(owner)->budget().checkpoint() ? 0 : 1;
}

void raw_tag(void* owner, int tag, int type, int quantity, unsigned int, void* input, INT64) {
    auto& reader = *static_cast<formats::Reader*>(owner);
    if (quantity < 0 || input == nullptr || dynamic_cast<RawStream*>(static_cast<LibRaw_abstract_datastream*>(input)) == nullptr) { return; }
    const auto width = type == 1 || type == 2 || type == 6 || type == 7 ? 1U : type == 3 || type == 8 ? 2U : type == 4 || type == 9 || type == 11 || type == 13 ? 4U : type == 5 || type == 10 || type == 12 ? 8U : 0U;
    const auto position = static_cast<LibRaw_abstract_datastream*>(input)->tell();
    if (width == 0 || position < 0) { return; }
    try {
        reader.entry();
        const auto length = static_cast<std::uint64_t>(quantity) * width;
        const auto code = static_cast<std::uint32_t>(tag);
        reader.put(formats::hex_key(code), ValueKind::Bytes, "", static_cast<std::uint64_t>(position), reader.read(static_cast<std::uint64_t>(position), length), "", code, true);
    } catch (const formats::Failure&) {}
}

template<std::size_t Size>
void library_text(formats::Reader& reader, const std::string& key, const char (&value)[Size]) {
    const auto end = std::find(value, value + Size, '\0');
    if (end != value) { reader.decoded(key, ValueKind::Text, std::string(value, end), "raw", 2); }
}

class RawParser final : public Parser {
public:
    const ParserCapability& capability() const noexcept override {
        static const ParserCapability value{"images.raw", "LibRaw camera metadata adapter", LIBRAW_VERSION_STR,
            Probe | Extract, {"dng", "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "raf", "orf", "rw2", "pef", "rwl", "x3f", "kdc", "dcr", "mos", "mef", "3fr", "iiq", "erf"}};
        return value;
    }
    ParseStatus run(ParserContext& context) const override {
        using namespace formats;
        Reader reader(context, 0, context.source_size(), "image.raw");
        try {
            reader.entry();
            if (reader.size < 16) { return ParseStatus::Unsupported; }
            const auto prefix = reader.read(0, 8);
            if ((starts(prefix, "II") || starts(prefix, "MM")) &&
                (integer(prefix, 2, 2, starts(prefix, "II")) == 42 || integer(prefix, 2, 2, starts(prefix, "II")) == 43)) {
                auto original = reader.window(0, reader.size, reader.group + ".original"); inspect_tiff(original, true);
            }
            if (!context.budget().reserve_allocation(sizeof(LibRaw) + 16ULL * 1024 * 1024)) { throw Failure{ParseStatus::Internal, "RAW library working-memory budget", 0}; }
            RawStream stream(reader);
            auto raw = std::make_unique<LibRaw>();
            raw->imgdata.rawparams.max_raw_memory_mb = 16;
            raw->set_progress_handler(progress, &context);
            raw->set_exifparser_handler(raw_tag, &reader);
            raw->set_makernotes_handler(raw_tag, &reader);
            const auto status = raw->open_datastream(&stream);
            if (status != LIBRAW_SUCCESS) {
                reader.diagnostic("library_error", LibRaw::strerror(status), 0);
                return status == LIBRAW_FILE_UNSUPPORTED ? ParseStatus::Unsupported : ParseStatus::Malformed;
            }
            const auto& data = raw->imgdata;
            const auto number = [&](const std::string& key, const auto value) { reader.decoded(key, ValueKind::Unsigned, std::to_string(value), "raw", 2); };
            const auto real = [&](const std::string& key, const auto value) { std::ostringstream out; out << std::setprecision(17) << value; reader.decoded(key, ValueKind::Float, out.str(), "raw", 2); };
            library_text(reader, "make", data.idata.make); library_text(reader, "model", data.idata.model);
            library_text(reader, "normalized_make", data.idata.normalized_make); library_text(reader, "normalized_model", data.idata.normalized_model);
            library_text(reader, "software", data.idata.software); library_text(reader, "artist", data.other.artist);
            library_text(reader, "description", data.other.desc); library_text(reader, "lens_model", data.lens.Lens);
            library_text(reader, "lens_make", data.lens.LensMake); library_text(reader, "lens_serial_number", data.lens.LensSerial);
            library_text(reader, "body_serial_number", data.shootinginfo.BodySerial); library_text(reader, "internal_body_serial", data.shootinginfo.InternalBodySerial);
            number("raw_count", data.idata.raw_count); number("dng_version", data.idata.dng_version);
            number("sensor_width", data.sizes.raw_width); number("sensor_height", data.sizes.raw_height);
            number("width", data.sizes.width); number("height", data.sizes.height);
            number("colors", data.idata.colors); number("cfa_filters", data.idata.filters);
            number("top_margin", data.sizes.top_margin); number("left_margin", data.sizes.left_margin);
            number("thumbnail_width", data.thumbnail.twidth); number("thumbnail_height", data.thumbnail.theight);
            reader.decoded("libraw_flip", ValueKind::Signed, std::to_string(data.sizes.flip), "raw", 2);
            real("pixel_aspect", data.sizes.pixel_aspect); real("iso", data.other.iso_speed);
            real("exposure_seconds", data.other.shutter); real("f_number", data.other.aperture); real("focal_length_mm", data.other.focal_len);
            real("camera_temperature", data.makernotes.common.CameraTemperature);
            reader.decoded("timestamp_library", ValueKind::Signed, std::to_string(data.other.timestamp), "raw", 2);
            reader.decoded("lens_id", ValueKind::Unsigned, std::to_string(data.lens.makernotes.LensID), "raw", 2);
            reader.decoded("drive_mode", ValueKind::Signed, std::to_string(data.shootinginfo.DriveMode), "raw", 2);
            reader.decoded("focus_mode", ValueKind::Signed, std::to_string(data.shootinginfo.FocusMode), "raw", 2);
            reader.decoded("metering_mode", ValueKind::Signed, std::to_string(data.shootinginfo.MeteringMode), "raw", 2);
            if (data.other.parsed_gps.gpsparsed != 0) {
                for (unsigned index = 0; index < 3; ++index) {
                    real("gps_latitude_component" + std::to_string(index), data.other.parsed_gps.latitude[index]);
                    real("gps_longitude_component" + std::to_string(index), data.other.parsed_gps.longitude[index]);
                }
                real("gps_altitude", data.other.parsed_gps.altitude);
            }
            if (data.idata.xmpdata != nullptr && data.idata.xmplen != 0) {
                auto xmp = std::span(reinterpret_cast<const std::uint8_t*>(data.idata.xmpdata), data.idata.xmplen);
                reader.decoded("xmp_packet_library", ValueKind::DecodedBytes, std::string(xmp.begin(), xmp.end()), "raw", 2);
                inspect_xmp_data(reader, xmp, "raw");
            }
            return ParseStatus::Complete;
        } catch (const Failure& failure) {
            if (context.budget().status() == BudgetStatus::Ok) { reader.diagnostic("raw_error", failure.message, 0); }
            return failure.status;
        }
    }
};

}

std::unique_ptr<Parser> raw_metadata_parser() { return std::make_unique<RawParser>(); }

}
