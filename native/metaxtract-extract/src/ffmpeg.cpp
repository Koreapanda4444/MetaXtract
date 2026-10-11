#include "format.hpp"

extern "C" {
#include <libavformat/avformat.h>
#include <libavcodec/avcodec.h>
#include <libavutil/avutil.h>
#include <libavutil/imgutils.h>
#include <libavutil/samplefmt.h>
#include <libavutil/pixdesc.h>
#include <libswscale/swscale.h>
}
#include <memory>
#include <mutex>
#include <cerrno>

namespace metaxtract::native::formats {
namespace {
struct IoState {
    Reader& reader;
    std::int64_t position = 0;
    ParseStatus failure = ParseStatus::Complete;
    bool external_refused = false;
};
int read_packet(void* owner, std::uint8_t* destination, int maximum) noexcept {
    auto& io = *static_cast<IoState*>(owner);
    try {
        io.reader.checkpoint();
        if (maximum <= 0 || io.position < 0) { return AVERROR(EINVAL); }
        if (static_cast<std::uint64_t>(io.position) >= io.reader.size) { return AVERROR_EOF; }
        const auto count = std::min<std::uint64_t>({static_cast<std::uint64_t>(maximum), io.reader.size - static_cast<std::uint64_t>(io.position), 65536});
        const auto data = io.reader.read(static_cast<std::uint64_t>(io.position), count);
        std::memcpy(destination, data.data(), data.size());
        io.position += static_cast<std::int64_t>(count);
        return static_cast<int>(count);
    } catch (const Failure& error) { io.failure = error.status; }
    catch (const std::bad_alloc&) { io.failure = ParseStatus::AllocationFailure; }
    catch (...) { io.failure = ParseStatus::Internal; }
    return AVERROR(EIO);
}
std::int64_t seek_packet(void* owner, std::int64_t offset, int whence) noexcept {
    auto& io = *static_cast<IoState*>(owner);
    try {
        io.reader.checkpoint();
        if (whence & AVSEEK_SIZE) { return static_cast<std::int64_t>(io.reader.size); }
        whence &= ~AVSEEK_FORCE;
        const std::int64_t base = whence == SEEK_SET ? 0 : whence == SEEK_CUR ? io.position : whence == SEEK_END ? static_cast<std::int64_t>(io.reader.size) : -1;
        if (base < 0 || (offset < 0 && offset < -base) || (offset >= 0 && offset > static_cast<std::int64_t>(io.reader.size) - base)) { return AVERROR(EINVAL); }
        io.position = base + offset;
        return io.position;
    } catch (const Failure& error) { io.failure = error.status; }
    catch (...) { io.failure = ParseStatus::Internal; }
    return AVERROR(EIO);
}
int interrupted(void* owner) noexcept { return static_cast<IoState*>(owner)->reader.context.budget().checkpoint() ? 0 : 1; }
int deny_open(AVFormatContext* context, AVIOContext**, const char*, int, AVDictionary**) noexcept {
    if (context->opaque) { static_cast<IoState*>(context->opaque)->external_refused = true; }
    return AVERROR(EPERM);
}
class Session final {
public:
    IoState state;
    AVIOContext* io = nullptr;
    AVFormatContext* format = nullptr;
    explicit Session(Reader& reader) : state{reader} {}
    ~Session() {
        if (format) { avformat_close_input(&format); }
        if (io) { av_freep(&io->buffer); avio_context_free(&io); }
    }
    int open() {
        static std::once_flag initialized;
        std::call_once(initialized, [] { av_max_alloc(64U * 1024U * 1024U); });
        if (!state.reader.context.budget().reserve_allocation(32U * 1024U * 1024U + 32768U)) { return AVERROR(ENOMEM); }
        auto* buffer = static_cast<unsigned char*>(av_malloc(32768));
        if (!buffer) { return AVERROR(ENOMEM); }
        io = avio_alloc_context(buffer, 32768, 0, &state, read_packet, nullptr, seek_packet);
        if (!io) { av_free(buffer); return AVERROR(ENOMEM); }
        format = avformat_alloc_context();
        if (!format) { return AVERROR(ENOMEM); }
        format->pb = io; format->flags |= AVFMT_FLAG_CUSTOM_IO;
        format->opaque = &state; format->io_open = deny_open;
        format->interrupt_callback = {interrupted, &state};
        format->probesize = 1024 * 1024; format->max_analyze_duration = 5 * AV_TIME_BASE;
        format->max_streams = 4096; format->max_index_size = 1024 * 1024;
        AVDictionary* options = nullptr;
        av_dict_set(&options, "protocol_whitelist", "", 0);
        const auto result = avformat_open_input(&format, "", nullptr, &options);
        av_dict_free(&options);
        return result;
    }
};
void library(Reader& r, const std::string& key, ValueKind kind, std::string value, const std::string& stream) { r.decoded(key, kind, std::move(value), stream, 2); }
void number(Reader& r, const std::string& key, std::int64_t value, const std::string& stream) { library(r, key, ValueKind::Signed, std::to_string(value), stream); }
void rational(Reader& r, const std::string& key, AVRational value, const std::string& stream) {
    if (value.den > 0 && value.num >= 0) { library(r, key, ValueKind::Rational, std::to_string(value.num) + "/" + std::to_string(value.den), stream); }
}
void binary(Reader& r, const std::string& key, const std::uint8_t* bytes, std::size_t length, const std::string& stream) {
    if (length && !bytes) { throw Failure{ParseStatus::Malformed, "Media binary data is missing", r.base}; }
    if (!r.context.budget().reserve_allocation(length)) { throw Failure{ParseStatus::Internal, "Media binary copy budget exceeded", r.base}; }
    r.decoded(key, ValueKind::DecodedBytes, length ? std::string(reinterpret_cast<const char*>(bytes), length) : std::string{}, stream, 2, "");
}
void dictionary(Reader& r, AVDictionary* data, const std::string& stream) {
    const AVDictionaryEntry* entry = nullptr;
    while ((entry = av_dict_get(data, "", entry, AV_DICT_IGNORE_SUFFIX))) {
        r.entry(); library(r, "metadata_key", ValueKind::Text, entry->key, stream);
        library(r, "metadata." + path_component(entry->key), ValueKind::Text, entry->value, stream);
    }
}
void error_value(Reader& r, const std::string& key, int code, const std::string& stream) {
    char text[AV_ERROR_MAX_STRING_SIZE]{}; av_strerror(code, text, sizeof(text)); library(r, key, ValueKind::Text, text, stream);
}
void metadata(Reader& reader, AVFormatContext* format) {
    const std::string container = "ffmpeg.container";
    library(reader, "library_version", ValueKind::Text, av_version_info(), container);
    library(reader, "container_format", ValueKind::Text, format->iformat->name, container);
    number(reader, "stream_count", format->nb_streams, container);
    if (format->duration != AV_NOPTS_VALUE) { number(reader, "duration_us", format->duration, container); }
    if (format->start_time != AV_NOPTS_VALUE) { number(reader, "start_time_us", format->start_time, container); }
    if (format->bit_rate > 0) { number(reader, "bit_rate", format->bit_rate, container); }
    dictionary(reader, format->metadata, container);
    for (unsigned index = 0; index < format->nb_streams; ++index) {
        reader.entry(); auto* stream = format->streams[index]; const auto* p = stream->codecpar;
        const auto id = "ffmpeg.track" + std::to_string(index);
        Reader r(reader.context, 0, 0, "media.ffmpeg.track" + std::to_string(index));
        number(r, "stream_index", index, id); number(r, "stream_id", stream->id, id);
        library(r, "media_type", ValueKind::Text, av_get_media_type_string(p->codec_type) ? av_get_media_type_string(p->codec_type) : "unknown", id);
        library(r, "codec_name", ValueKind::Text, avcodec_get_name(p->codec_id), id);
        number(r, "codec_id", p->codec_id, id); number(r, "codec_tag", p->codec_tag, id);
        if (p->profile != AV_PROFILE_UNKNOWN) { number(r, "codec_profile", p->profile, id); if (const auto* profile = avcodec_profile_name(p->codec_id, p->profile)) { library(r, "profile_name", ValueKind::Text, profile, id); } }
        if (p->level != AV_LEVEL_UNKNOWN) { number(r, "codec_level", p->level, id); }
        rational(r, "time_base", stream->time_base, id); rational(r, "average_frame_rate", stream->avg_frame_rate, id); rational(r, "sample_aspect_ratio", p->sample_aspect_ratio, id);
        if (stream->start_time != AV_NOPTS_VALUE) { number(r, "start_time_ticks", stream->start_time, id); }
        if (stream->duration != AV_NOPTS_VALUE) { number(r, "duration_ticks", stream->duration, id); }
        if (stream->nb_frames) { number(r, "declared_frame_count", stream->nb_frames, id); }
        number(r, "disposition", stream->disposition, id);
        if (p->bit_rate > 0) { number(r, "bit_rate", p->bit_rate, id); }
        if (p->width > 0) { number(r, "width", p->width, id); } if (p->height > 0) { number(r, "height", p->height, id); }
        if (p->sample_rate > 0) { number(r, "sample_rate", p->sample_rate, id); } if (p->ch_layout.nb_channels > 0) { number(r, "channels", p->ch_layout.nb_channels, id); }
        if (p->bits_per_coded_sample > 0) { number(r, "bits_per_coded_sample", p->bits_per_coded_sample, id); } if (p->bits_per_raw_sample > 0) { number(r, "bits_per_raw_sample", p->bits_per_raw_sample, id); }
        if (p->codec_type == AVMEDIA_TYPE_VIDEO) {
            number(r, "color_range", p->color_range, id); number(r, "color_primaries", p->color_primaries, id);
            number(r, "color_transfer", p->color_trc, id); number(r, "color_space", p->color_space, id); number(r, "chroma_location", p->chroma_location, id);
        }
        if (p->extradata_size > 0) { binary(r, "codec_private", p->extradata, static_cast<std::size_t>(p->extradata_size), id); }
        dictionary(r, stream->metadata, id);
        if ((stream->disposition & AV_DISPOSITION_ATTACHED_PIC) && stream->attached_pic.size > 0) { binary(r, "attached_picture", stream->attached_pic.data, static_cast<std::size_t>(stream->attached_pic.size), id); }
    }
    for (unsigned index = 0; index < format->nb_chapters; ++index) {
        reader.entry(); const auto* chapter = format->chapters[index]; const auto id = "ffmpeg.chapter" + std::to_string(index);
        Reader r(reader.context, 0, 0, "media.ffmpeg.chapter" + std::to_string(index));
        number(r, "chapter_id", chapter->id, id); number(r, "chapter_start_ticks", chapter->start, id); number(r, "chapter_end_ticks", chapter->end, id);
        rational(r, "time_base", chapter->time_base, id); dictionary(r, chapter->metadata, id);
    }
    for (unsigned index = 0; index < format->nb_programs; ++index) {
        reader.entry(); const auto* program = format->programs[index]; const auto id = "ffmpeg.program" + std::to_string(index);
        Reader r(reader.context, 0, 0, "media.ffmpeg.program" + std::to_string(index));
        number(r, "program_id", program->id, id); number(r, "program_number", program->program_num, id); number(r, "pmt_pid", program->pmt_pid, id); number(r, "pcr_pid", program->pcr_pid, id); dictionary(r, program->metadata, id);
    }
}
int frame_buffer(AVCodecContext* codec, AVFrame* frame, int flags) noexcept {
    auto& state = *static_cast<IoState*>(codec->opaque);
    try {
        state.reader.checkpoint(); std::uint64_t bytes = 0;
        if (codec->codec_type == AVMEDIA_TYPE_VIDEO) {
            int width = frame->width, height = frame->height, alignment[AV_NUM_DATA_POINTERS]{};
            avcodec_align_dimensions2(codec, &width, &height, alignment);
            const auto size = av_image_get_buffer_size(static_cast<AVPixelFormat>(frame->format), width, height, 64);
            if (size < 0) { return size; } bytes = static_cast<std::uint64_t>(size) + 65536;
        } else {
            const auto size = av_samples_get_buffer_size(nullptr, frame->ch_layout.nb_channels, frame->nb_samples, static_cast<AVSampleFormat>(frame->format), 64);
            if (size < 0) { return size; } bytes = static_cast<std::uint64_t>(size) + 65536;
        }
        if (!state.reader.context.budget().reserve_allocation(bytes)) { return AVERROR(ENOMEM); }
        return avcodec_default_get_buffer2(codec, frame, flags);
    } catch (const Failure& error) { state.failure = error.status; }
    catch (...) { state.failure = ParseStatus::Internal; }
    return AVERROR(EIO);
}
struct CodecOwner { void operator()(AVCodecContext* p) const noexcept { avcodec_free_context(&p); } };
struct PacketOwner { void operator()(AVPacket* p) const noexcept { av_packet_free(&p); } };
struct FrameOwner { void operator()(AVFrame* p) const noexcept { av_frame_free(&p); } };
struct ScaleOwner { void operator()(SwsContext* p) const noexcept { sws_freeContext(p); } };
void decoded_frame(Reader& reader, const AVFrame& frame, unsigned index, const AVRational time_base) {
    const auto id = "ffmpeg.track" + std::to_string(index);
    Reader r(reader.context, 0, 0, "media.ffmpeg.track" + std::to_string(index) + ".decoded");
    number(r, "decoded_frame_count", 1, id);
    if (frame.best_effort_timestamp != AV_NOPTS_VALUE) { number(r, "frame_time_ticks", frame.best_effort_timestamp, id); rational(r, "time_base", time_base, id); }
    if (frame.width > 0 && frame.height > 0) {
        number(r, "decoded_width", frame.width, id); number(r, "decoded_height", frame.height, id);
        if (const auto* name = av_get_pix_fmt_name(static_cast<AVPixelFormat>(frame.format))) { library(r, "decoded_pixel_format", ValueKind::Text, name, id); }
        const auto longest = std::max(frame.width, frame.height);
        const auto width = static_cast<int>(std::max<std::int64_t>(1, static_cast<std::int64_t>(frame.width) * std::min(longest, 256) / longest));
        const auto height = static_cast<int>(std::max<std::int64_t>(1, static_cast<std::int64_t>(frame.height) * std::min(longest, 256) / longest));
        const auto length = static_cast<std::uint64_t>(width) * height * 3;
        if (!reader.context.budget().reserve_allocation(length + 64 + 1024 * 1024)) { throw Failure{ParseStatus::Internal, "Preview working budget exceeded", 0}; }
        std::vector<std::uint8_t> pixels(static_cast<std::size_t>(length) + 64);
        std::unique_ptr<SwsContext, ScaleOwner> scale(sws_getContext(frame.width, frame.height, static_cast<AVPixelFormat>(frame.format), width, height, AV_PIX_FMT_RGB24, SWS_BILINEAR, nullptr, nullptr, nullptr));
        if (!scale) { throw Failure{ParseStatus::Unsupported, "Pixel format cannot be converted to a preview", 0}; }
        std::uint8_t* output[] = {pixels.data(), nullptr, nullptr, nullptr}; const int strides[] = {width * 3, 0, 0, 0};
        if (sws_scale(scale.get(), frame.data, frame.linesize, 0, frame.height, output, strides) != height) { throw Failure{ParseStatus::Malformed, "Preview conversion did not produce all rows", 0}; }
        number(r, "preview_width", width, id); number(r, "preview_height", height, id); library(r, "preview_format", ValueKind::Text, "RGB24", id);
        binary(r, "preview_pixels", pixels.data(), static_cast<std::size_t>(length), id);
    } else if (frame.nb_samples > 0) {
        const auto format = static_cast<AVSampleFormat>(frame.format); const auto channels = frame.ch_layout.nb_channels;
        const auto sample_bytes = av_get_bytes_per_sample(format); if (channels <= 0 || sample_bytes <= 0) { throw Failure{ParseStatus::Malformed, "Decoded audio layout is invalid", 0}; }
        const auto planar = av_sample_fmt_is_planar(format) != 0; const auto planes = planar ? channels : 1;
        number(r, "decoded_samples", frame.nb_samples, id); number(r, "decoded_channels", channels, id); number(r, "decoded_sample_rate", frame.sample_rate, id);
        if (const auto* name = av_get_sample_fmt_name(format)) { library(r, "decoded_sample_format", ValueKind::Text, name, id); }
        const auto bytes = static_cast<std::uint64_t>(frame.nb_samples) * sample_bytes * (planar ? 1 : channels);
        for (int plane = 0; plane < planes; ++plane) {
            r.entry(); Reader node(r.context, 0, 0, r.group + ".plane" + std::to_string(plane));
            number(node, "plane_index", plane, id); binary(node, "audio_samples", frame.extended_data[plane], static_cast<std::size_t>(bytes), id);
        }
    }
}
ParseStatus decode(Session& session, Reader& reader) {
    struct Track { std::unique_ptr<AVCodecContext, CodecOwner> codec; bool completed = false; };
    std::vector<Track> tracks(session.format->nb_streams);
    bool failed = false; unsigned expected = 0, completed = 0;
    for (unsigned index = 0; index < tracks.size(); ++index) {
        reader.entry(); const auto* p = session.format->streams[index]->codecpar;
        if (p->codec_type != AVMEDIA_TYPE_VIDEO && p->codec_type != AVMEDIA_TYPE_AUDIO) { continue; }
        const auto id = "ffmpeg.track" + std::to_string(index); const auto* decoder = avcodec_find_decoder(p->codec_id);
        if (!decoder) { library(reader, "decode_unavailable", ValueKind::Text, avcodec_get_name(p->codec_id), id); failed = true; continue; }
        auto& track = tracks[index]; track.codec.reset(avcodec_alloc_context3(decoder));
        if (!track.codec) { return ParseStatus::AllocationFailure; }
        auto status = avcodec_parameters_to_context(track.codec.get(), p);
        track.codec->thread_count = 1; track.codec->max_pixels = 16 * 1024 * 1024;
        track.codec->opaque = &session.state; track.codec->get_buffer2 = frame_buffer;
        if (status >= 0) { status = avcodec_open2(track.codec.get(), decoder, nullptr); }
        if (status < 0) { error_value(reader, "decode_open_error", status, id); track.codec.reset(); failed = true; continue; }
        ++expected;
    }
    std::unique_ptr<AVPacket, PacketOwner> packet(av_packet_alloc()); std::unique_ptr<AVFrame, FrameOwner> frame(av_frame_alloc());
    if (!packet || !frame) { return ParseStatus::AllocationFailure; }
    const auto receive = [&](unsigned index) {
        auto& track = tracks[index];
        while (!track.completed) {
            reader.checkpoint(); const auto status = avcodec_receive_frame(track.codec.get(), frame.get());
            if (status == AVERROR(EAGAIN) || status == AVERROR_EOF) { break; }
            if (status < 0) { error_value(reader, "decode_error", status, "ffmpeg.track" + std::to_string(index)); track.completed = true; ++completed; failed = true; break; }
            decoded_frame(reader, *frame, index, session.format->streams[index]->time_base); av_frame_unref(frame.get());
            track.completed = true; ++completed;
        }
    };
    while (completed < expected) {
        reader.entry(); const auto status = av_read_frame(session.format, packet.get());
        if (status < 0) { if (status != AVERROR_EOF) { error_value(reader, "demux_error", status, "ffmpeg.container"); failed = true; } break; }
        if (packet->stream_index >= 0 && static_cast<std::size_t>(packet->stream_index) < tracks.size()) {
            const auto index = static_cast<unsigned>(packet->stream_index); auto& track = tracks[index];
            if (track.codec && !track.completed) {
                auto sent = avcodec_send_packet(track.codec.get(), packet.get());
                if (sent == AVERROR(EAGAIN)) { receive(index); if (!track.completed) { sent = avcodec_send_packet(track.codec.get(), packet.get()); } }
                if (sent < 0 && sent != AVERROR(EAGAIN)) { error_value(reader, "packet_decode_error", sent, "ffmpeg.track" + std::to_string(index)); track.completed = true; ++completed; failed = true; }
                else { receive(index); }
            }
        }
        av_packet_unref(packet.get());
    }
    for (unsigned index = 0; index < tracks.size(); ++index) {
        auto& track = tracks[index]; if (!track.codec || track.completed) { continue; }
        avcodec_send_packet(track.codec.get(), nullptr); receive(index);
        if (!track.completed) { library(reader, "decode_no_frame", ValueKind::Text, "No complete frame was decoded", "ffmpeg.track" + std::to_string(index)); failed = true; }
    }
    if (!expected && !completed) { return ParseStatus::Unsupported; }
    return failed ? ParseStatus::Unsupported : ParseStatus::Complete;
}
class MediaLibraryParser final : public Parser {
    bool decode_;
public:
    explicit MediaLibraryParser(bool preview) : decode_(preview) {}
    const ParserCapability& capability() const noexcept override {
        static const ParserCapability inspect{"media.inspect", "In-process FFmpeg media inspection", "9.0.2", Probe | Extract, {"mp4","mov","matroska","webm","avi","mpeg","mpeg_ts","mp3","flac","ogg","opus","wav","aiff","ape","wavpack","asf","mxf","jpeg","png"}};
        static const ParserCapability preview{"media.decode", "Bounded in-process FFmpeg preview decoding", "9.0.2", Extract | DecodePreview, inspect.formats};
        return decode_ ? preview : inspect;
    }
    ParseStatus run(ParserContext& context) const override {
        if (context.source_size() == 0 || context.source_size() > static_cast<std::uint64_t>(INT64_MAX)) { return ParseStatus::Unsupported; }
        Reader reader(context, 0, context.source_size(), "media.ffmpeg");
        try {
            Session session(reader); const auto opened = session.open();
            if (opened < 0) {
                if (session.state.external_refused) { library(reader, "external_reference_refused", ValueKind::Text, "External resource opening is forbidden", "ffmpeg.container"); }
                error_value(reader, "open_error", opened, "ffmpeg.container");
                return session.state.failure != ParseStatus::Complete ? session.state.failure : opened == AVERROR(ENOMEM) ? ParseStatus::AllocationFailure : ParseStatus::Unsupported;
            }
            const auto info = avformat_find_stream_info(session.format, nullptr);
            reader.checkpoint(); metadata(reader, session.format);
            if (session.state.external_refused) { library(reader, "external_reference_refused", ValueKind::Text, "External resource opening is forbidden", "ffmpeg.container"); }
            if (session.state.failure != ParseStatus::Complete) { return session.state.failure; }
            if (info < 0) { error_value(reader, "stream_info_error", info, "ffmpeg.container"); return ParseStatus::Unsupported; }
            if (decode_) {
                const auto status = decode(session, reader);
                return session.state.failure == ParseStatus::Complete ? status : session.state.failure;
            }
            return ParseStatus::Complete;
        } catch (const Failure& failure) { reader.diagnostic("failure", failure.message, std::min(reader.size, failure.offset)); return failure.status; }
    }
};
}
}
namespace metaxtract::native {
std::unique_ptr<Parser> media_inspection_parser() { return std::make_unique<formats::MediaLibraryParser>(false); }
std::unique_ptr<Parser> media_decoding_parser() { return std::make_unique<formats::MediaLibraryParser>(true); }
}
