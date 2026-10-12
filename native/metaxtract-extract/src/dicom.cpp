#include "format.hpp"
#include "text.hpp"

#include <bit>
#include <cmath>
#include <set>

namespace metaxtract::native::formats {
namespace {
struct DicomTag { const char* name; const char* vr; };
DicomTag dicom_tag(std::uint32_t tag) {
    switch (tag) {
        case 0x00020000: return {"meta_group_length", "UL"};
        case 0x00020001: return {"meta_version", "OB"};
        case 0x00020002: return {"media_sop_class_uid", "UI"};
        case 0x00020003: return {"media_sop_instance_uid", "UI"};
        case 0x00020010: return {"transfer_syntax_uid", "UI"};
        case 0x00020012: return {"implementation_class_uid", "UI"};
        case 0x00020013: return {"implementation_version", "SH"};
        case 0x00020016: return {"source_application", "AE"};
        case 0x00080005: return {"character_set", "CS"};
        case 0x00080008: return {"image_type", "CS"};
        case 0x00080016: return {"sop_class_uid", "UI"};
        case 0x00080018: return {"sop_instance_uid", "UI"};
        case 0x00080020: return {"study_date", "DA"};
        case 0x00080021: return {"series_date", "DA"};
        case 0x00080022: return {"acquisition_date", "DA"};
        case 0x00080023: return {"content_date", "DA"};
        case 0x0008002a: return {"acquisition_datetime", "DT"};
        case 0x00080030: return {"study_time", "TM"};
        case 0x00080031: return {"series_time", "TM"};
        case 0x00080032: return {"acquisition_time", "TM"};
        case 0x00080033: return {"content_time", "TM"};
        case 0x00080050: return {"accession_number", "SH"};
        case 0x00080060: return {"modality", "CS"};
        case 0x00080070: return {"manufacturer", "LO"};
        case 0x00080080: return {"institution", "LO"};
        case 0x00080081: return {"institution_address", "ST"};
        case 0x00080090: return {"referring_physician", "PN"};
        case 0x00081030: return {"study_description", "LO"};
        case 0x0008103e: return {"series_description", "LO"};
        case 0x00081090: return {"device_model", "LO"};
        case 0x00081110: case 0x00081115: case 0x00081140: case 0x00082112: return {"reference_sequence", "SQ"};
        case 0x00100010: return {"patient_name", "PN"};
        case 0x00100020: return {"patient_id", "LO"};
        case 0x00100021: return {"patient_id_issuer", "LO"};
        case 0x00100030: return {"patient_birth_date", "DA"};
        case 0x00100040: return {"patient_sex", "CS"};
        case 0x00101010: return {"patient_age", "AS"};
        case 0x00180050: return {"slice_thickness", "DS"};
        case 0x00180080: return {"repetition_time", "DS"};
        case 0x00180081: return {"echo_time", "DS"};
        case 0x00181000: return {"device_serial_number", "LO"};
        case 0x00181020: return {"software_versions", "LO"};
        case 0x00181030: return {"protocol_name", "LO"};
        case 0x0020000d: return {"study_instance_uid", "UI"};
        case 0x0020000e: return {"series_instance_uid", "UI"};
        case 0x00200010: return {"study_id", "SH"};
        case 0x00200011: return {"series_number", "IS"};
        case 0x00200013: return {"instance_number", "IS"};
        case 0x00200032: return {"image_position", "DS"};
        case 0x00200037: return {"image_orientation", "DS"};
        case 0x00200052: return {"frame_of_reference_uid", "UI"};
        case 0x00280002: return {"samples_per_pixel", "US"};
        case 0x00280004: return {"photometric_interpretation", "CS"};
        case 0x00280006: return {"planar_configuration", "US"};
        case 0x00280008: return {"frame_count", "IS"};
        case 0x00280010: return {"rows", "US"};
        case 0x00280011: return {"columns", "US"};
        case 0x00280030: return {"pixel_spacing", "DS"};
        case 0x00280100: return {"bits_allocated", "US"};
        case 0x00280101: return {"bits_stored", "US"};
        case 0x00280102: return {"high_bit", "US"};
        case 0x00280103: return {"pixel_representation", "US"};
        case 0x00281050: return {"window_center", "DS"};
        case 0x00281051: return {"window_width", "DS"};
        case 0x00281052: return {"rescale_intercept", "DS"};
        case 0x00281053: return {"rescale_slope", "DS"};
        case 0x00400275: return {"request_attributes", "SQ"};
        case 0x52009229: case 0x52009230: return {"functional_groups", "SQ"};
        case 0x7fe00010: return {"pixel_data", "OW"};
        default: return {nullptr, "UN"};
    }
}
bool valid_vr(const std::string_view vr) {
    constexpr std::string_view all = " AE AS AT CS DA DS DT FL FD IS LO LT OB OD OF OL OV OW PN SH SL SQ SS ST SV TM UC UI UL UN UR US UT UV ";
    return vr.size() == 2 && all.find(" " + std::string(vr) + " ") != std::string_view::npos;
}
bool long_vr(const std::string_view vr) { return vr == "OB" || vr == "OD" || vr == "OF" || vr == "OL" || vr == "OV" || vr == "OW" || vr == "SQ" || vr == "UC" || vr == "UN" || vr == "UR" || vr == "UT"; }
struct DicomState { bool explicit_vr = true, little = true; std::string charset = "ASCII", transfer; };
std::uint64_t dnum(Reader& r, std::uint64_t at, unsigned width, bool little) { return integer(r.read(at, width), 0, width, little); }
std::uint32_t dtag(Reader& r, std::uint64_t at, bool little) { const auto bytes = r.read(at, 4); return static_cast<std::uint32_t>((integer(bytes, 0, 2, little) << 16U) | integer(bytes, 2, 2, little)); }
std::string trim_dicom(std::span<const std::uint8_t> bytes) { std::string text(bytes.begin(), bytes.end()); while (!text.empty() && (text.back() == '\0' || text.back() == ' ')) { text.pop_back(); } return text; }
struct Element { std::uint32_t tag; std::string vr; std::uint64_t header, payload, length; };
Element element(Reader& r, std::uint64_t at, const DicomState& state) {
    r.require(at, 8); const auto tag = dtag(r, at, state.little); auto vr = std::string(dicom_tag(tag).vr); std::uint64_t header = 8, length = 0;
    if (state.explicit_vr && (tag >> 16U) != 0xfffe) {
        const auto bytes = r.read(at + 4, 2); vr.assign(bytes.begin(), bytes.end());
        if (!valid_vr(vr)) { throw Failure{ParseStatus::Malformed, "Invalid DICOM value representation", r.base + at}; }
        header = long_vr(vr) ? 12 : 8;
        if (header == 12 && dnum(r, at + 6, 2, state.little)) { throw Failure{ParseStatus::Malformed, "DICOM reserved VR bytes are nonzero", r.base + at}; }
        length = dnum(r, at + (header == 12 ? 8 : 6), header == 12 ? 4 : 2, state.little);
    } else { length = dnum(r, at + 4, 4, state.little); }
    r.require(at, header); if (length != 0xffffffffU) { r.require(at + header, length); }
    return {tag, vr, header, at + header, length};
}
void dicom_value(Reader& r, const Element& item, DicomState& state) {
    const auto identity = dicom_tag(item.tag); const auto key = identity.name ? std::string(identity.name) : hex_key(item.tag);
    auto raw = r.read(item.payload, item.length); const auto& vr = item.vr;
    if (state.explicit_vr) { r.put("vr", ValueKind::Text, vr, item.payload - item.header + 4, r.read(item.payload - item.header + 4, 2), "ASCII"); }
    else { r.decoded("vr", ValueKind::Text, vr, r.group, 0, "ASCII"); }
    if (item.tag == 0x00020010) { state.transfer = trim_dicom(raw); }
    if (item.tag == 0x00080005) {
        const auto charset = trim_dicom(raw); state.charset = charset.empty() || charset == "ISO_IR 6" ? "ASCII" : charset == "ISO_IR 100" ? "ISO-8859-1" : charset == "ISO_IR 192" ? "UTF-8" : "unsupported-DICOM-charset";
        if (state.charset == "unsupported-DICOM-charset") { r.diagnostic("charset_unsupported", "Character set retained without speculative text conversion", item.payload); }
    }
    const bool text = vr == "AE" || vr == "AS" || vr == "CS" || vr == "DA" || vr == "DS" || vr == "DT" || vr == "IS" || vr == "LO" || vr == "LT" || vr == "PN" || vr == "SH" || vr == "ST" || vr == "TM" || vr == "UC" || vr == "UI" || vr == "UR" || vr == "UT";
    if (text) {
        const auto encoding = vr == "UI" || vr == "CS" || vr == "DA" || vr == "DS" || vr == "DT" || vr == "IS" || vr == "TM" ? "ASCII" : state.charset;
        const auto converted = decode_text(r, raw, encoding); auto value = converted.value;
        while (!value.empty() && (value.back() == '\0' || value.back() == ' ')) { value.pop_back(); }
        r.put(key, converted.valid && converted.supported ? ValueKind::Text : ValueKind::Bytes, value, item.payload, std::move(raw), encoding, item.tag, true); return;
    }
    const unsigned width = vr == "US" || vr == "SS" ? 2 : vr == "UL" || vr == "SL" || vr == "FL" ? 4 : vr == "UV" || vr == "SV" || vr == "FD" ? 8 : 0;
    if (!width || raw.empty() || raw.size() % width) { r.put(key, ValueKind::Bytes, "", item.payload, std::move(raw), "binary", item.tag, true); return; }
    const bool sign = vr == "SS" || vr == "SL" || vr == "SV", floating = vr == "FL" || vr == "FD";
    const auto capacity = static_cast<std::uint64_t>(raw.size() / width) * 32;
    if (!r.context.budget().reserve_allocation(capacity) || !r.context.budget().consume_decoded(capacity)) { throw Failure{ParseStatus::Internal, "DICOM numeric decoding budget", item.payload}; }
    std::string values; bool finite = true;
    for (std::size_t at = 0; at < raw.size(); at += width) {
        r.entry(); const auto value = integer(raw, at, width, state.little); if (at) { values += ','; }
        if (floating) { const auto number = width == 4 ? static_cast<double>(std::bit_cast<float>(static_cast<std::uint32_t>(value))) : std::bit_cast<double>(value); finite = finite && std::isfinite(number); std::ostringstream out; out << std::setprecision(17) << number; values += out.str(); }
        else if (sign) { values += std::to_string(width == 2 ? std::bit_cast<std::int16_t>(static_cast<std::uint16_t>(value)) : width == 4 ? std::bit_cast<std::int32_t>(static_cast<std::uint32_t>(value)) : std::bit_cast<std::int64_t>(value)); }
        else { values += std::to_string(value); }
    }
    const auto kind = !finite ? ValueKind::Bytes : floating ? (raw.size() == width ? ValueKind::Float : ValueKind::FloatSequence) : sign ? (raw.size() == width ? ValueKind::Signed : ValueKind::SignedSequence) : (raw.size() == width ? ValueKind::Unsigned : ValueKind::UnsignedSequence);
    r.put(key, kind, std::move(values), item.payload, std::move(raw), "", item.tag, true);
}

std::uint64_t dataset(Reader& r, std::uint64_t at, DicomState state, std::uint32_t depth, std::uint32_t stop_tag = 0);
std::uint64_t sequence(Reader& r, std::uint64_t at, std::uint64_t end, DicomState state, std::uint32_t depth, bool undefined, bool pixels) {
    std::uint64_t index = 0;
    while (at < end) {
        r.entry(); if (end - at < 8) { throw Failure{ParseStatus::Malformed, "DICOM item header escapes its sequence", r.base + at}; }
        const auto tag = dtag(r, at, state.little); const auto size = dnum(r, at + 4, 4, state.little);
        if (tag == 0xfffee0dd && undefined && size == 0) { return at + 8; }
        if (tag != 0xfffee000) { throw Failure{ParseStatus::Malformed, "DICOM sequence requires an item", r.base + at}; }
        auto item = r; item.group += ".item" + std::to_string(index++); const auto payload = at + 8;
        if (size != 0xffffffffU && size > end - payload) { throw Failure{ParseStatus::Malformed, "DICOM item exceeds its sequence", r.base + at}; }
        if (pixels) {
            if (size == 0xffffffffU) { throw Failure{ParseStatus::Malformed, "Pixel fragment has undefined length", r.base + at}; }
            r.require(payload, size); item.number("fragment_size", at + 4, 4, state.little);
            if (index == 1) { item.put("basic_offset_table", ValueKind::Bytes, "", payload, r.read(payload, size)); }
            at = payload + size;
        } else if (size == 0xffffffffU) { auto content = item.window(payload, end - payload, item.group); at = payload + dataset(content, 0, state, depth + 1, 0xfffee00d); }
        else { auto content = item.window(payload, size, item.group); static_cast<void>(dataset(content, 0, state, depth + 1)); at = payload + size; }
        if (at > end) { throw Failure{ParseStatus::Malformed, "DICOM item escapes its sequence", r.base + at}; }
    }
    if (undefined) { throw Failure{ParseStatus::Malformed, "DICOM sequence delimiter is missing", r.base + at}; }
    return at;
}
std::uint64_t dataset(Reader& r, std::uint64_t at, DicomState state, std::uint32_t depth, std::uint32_t stop_tag) {
    if (!r.context.budget().check_depth(depth)) { throw Failure{ParseStatus::Internal, "DICOM sequence depth limit", r.base}; }
    std::uint64_t index = 0; std::set<std::uint32_t> seen;
    while (at < r.size) {
        r.entry(); const auto item = element(r, at, state);
        if (item.tag == stop_tag && stop_tag && item.length == 0) { return item.payload; }
        if ((item.tag >> 16U) == 0xfffe) { throw Failure{ParseStatus::Malformed, "Unexpected DICOM delimiter", r.base + at}; }
        if (!r.context.budget().reserve_allocation(64)) { throw Failure{ParseStatus::Internal, "DICOM element index allocation limit", r.base + at}; }
        auto field = r; field.group += ".element" + std::to_string(index++); field.derived("tag", item.tag); field.derived("declared_length", item.length);
        if (!seen.insert(item.tag).second) { field.diagnostic("duplicate_tag", "Repeated DICOM tag retained as a distinct observation", at); }
        if (item.vr == "SQ" || item.length == 0xffffffffU) {
            const bool undefined = item.length == 0xffffffffU; const auto end = undefined ? r.size : item.payload + item.length;
            at = sequence(field, item.payload, end, state, depth, undefined, item.tag == 0x7fe00010);
        } else {
            if (item.tag == 0x7fe00010 || item.tag == 0x7fe00008 || item.tag == 0x7fe00009) { field.derived("pixel_data_offset", item.payload); field.derived("pixel_data_length", item.length); }
            else { dicom_value(field, item, state); }
            at = item.payload + item.length;
        }
    }
    if (stop_tag) { throw Failure{ParseStatus::Malformed, "DICOM item delimiter is missing", r.base + at}; } return at;
}
ParseStatus dicom(Reader& r) {
    DicomState state; std::uint64_t at = 0;
    const bool preamble = r.size >= 132 && starts(r.read(128, 4), "DICM");
    if (preamble) {
        r.put("preamble", ValueKind::Bytes, "", 0, r.read(0, 128)); at = 132;
        while (at + 8 <= r.size && dtag(r, at, true) >> 16U == 2) { r.entry(); const auto item = element(r, at, state); if (item.length == 0xffffffffU) { throw Failure{ParseStatus::Malformed, "Undefined DICOM file meta length", at}; } auto meta = r; meta.group = "dicom.meta"; dicom_value(meta, item, state); at = item.payload + item.length; }
        if (state.transfer.empty()) { throw Failure{ParseStatus::Malformed, "DICOM transfer syntax UID is missing", at}; }
    } else {
        if (r.size < 8) { return ParseStatus::Unsupported; }
        const auto group = dtag(r, 0, true) >> 16U; if (group != 8 && group != 16 && group != 24 && group != 32 && group != 40) { return ParseStatus::Unsupported; }
        const auto vr = r.read(4, 2); state.explicit_vr = valid_vr({reinterpret_cast<const char*>(vr.data()), vr.size()});
        r.diagnostic("transfer_syntax_assumed", "Raw dataset has no file meta; little endian VR encoding is inferred from its header", 0);
    }
    if (state.transfer == "1.2.840.10008.1.2") { state.explicit_vr = false; }
    else if (state.transfer == "1.2.840.10008.1.2.2") { state.little = false; }
    else if (state.transfer == "1.2.840.10008.1.2.1.99") {
        const auto compressed = r.read(at, r.size - at); const auto bytes = inflate_metadata(r, compressed, -15); Reader decoded(r.context, bytes, "dicom.dataset", "dicom.deflated"); static_cast<void>(dataset(decoded, 0, state, 0)); return ParseStatus::Complete;
    } else if (!state.transfer.empty() && state.transfer != "1.2.840.10008.1.2.1" && !state.transfer.starts_with("1.2.840.10008.1.2.4.") && state.transfer != "1.2.840.10008.1.2.5") {
        r.diagnostic("transfer_syntax_unsupported", "Unknown transfer syntax retained; dataset encoding is not guessed", at); return ParseStatus::Unsupported;
    }
    static_cast<void>(dataset(r, at, state, 0)); return ParseStatus::Complete;
}
}
}
namespace metaxtract::native {
namespace {
class DicomParser final : public Parser {
    const ParserCapability& capability() const noexcept override { static const ParserCapability value{"formats.dicom", "DICOM metadata and private data elements", "1", Extract, {"dcm", "dicom"}}; return value; }
    ParseStatus run(ParserContext& context) const override {
        formats::Reader r(context, 0, context.source_size(), "dicom");
        try { return formats::dicom(r); } catch (const formats::Failure& error) { r.diagnostic("failure", error.message, 0); return error.status; }
    }
};
}
std::unique_ptr<Parser> dicom_metadata_parser() { return std::make_unique<DicomParser>(); }
}
