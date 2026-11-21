#include "format.hpp"
#include <array>
#include <memory>
#include <zlib.h>

namespace metaxtract::native {
namespace {
using namespace formats;
constexpr std::string_view uuid_tail("\0\x11\0\x10\x80\0\0\xaa\0\x38\x9b\x71",12);
struct Box { std::uint64_t at, size, header; std::string type; };
Box box(Reader& r, std::uint64_t at) {
    r.entry();auto h=r.read(at,8);auto size=integer(h,0,4);std::uint64_t header=8;
    if(size==1) {size=integer(r.read(at+8,8),0,8);header=16;}
    if(size==0 || size<header) {throw Failure{ParseStatus::Malformed,"Invalid JUMBF box length",at};}
    r.require(at,size);return {at,size,header,std::string(h.begin()+4,h.end())};
}
std::string hex(std::span<const std::uint8_t> bytes) {
    constexpr char digits[]="0123456789abcdef";std::string out;out.reserve(bytes.size()*2);
    for(auto b:bytes) {out.push_back(digits[b>>4U]);out.push_back(digits[b&15U]);}return out;
}
bool uuid(std::span<const std::uint8_t> bytes, std::string_view prefix) {
    return bytes.size()>=16 && starts(bytes,prefix) && starts(bytes,uuid_tail,4);
}
class Store final {
    Reader& packet_; unsigned store_; std::uint64_t next_=0;
    void walk(const Box& current, std::uint64_t parent, std::uint32_t depth) {
        packet_.checkpoint();if(depth>64) {throw Failure{ParseStatus::Unsupported,"JUMBF nesting exceeds supported depth",current.at};}if(!packet_.context.budget().check_depth(depth)) {throw Failure{ParseStatus::Internal,"JUMBF depth limit",current.at};}
        auto body=packet_.window(current.at+current.header,current.size-current.header,packet_.group);
        const auto id=next_++;Reader fields=body;fields.group="c2pa.store"+std::to_string(store_)+".box"+std::to_string(id);
        fields.derived("box_index",id);fields.derived("parent_index",parent);fields.derived("box_offset",current.at);
        fields.derived("box_size",current.size);fields.derived("box_header_size",current.header);
        fields.decoded("box_type",ValueKind::Text,current.type,"c2pa-store"+std::to_string(store_));
        if(current.type!="jumb") {
            auto payload=body.read(0,body.size);fields.put("box_payload",ValueKind::Bytes,"",0,std::move(payload));return;
        }
        auto description=box(body,0);
        if(description.type!="jumd" || description.size-description.header<18) {throw Failure{ParseStatus::Malformed,"JUMBF description must be first",current.at};}
        const auto data=body.read(description.header,description.size-description.header);
        if((data[16]&3U)!=3U) {throw Failure{ParseStatus::Malformed,"C2PA JUMBF label and requestable toggles required",current.at};}
        const auto end=std::find(data.begin()+17,data.end(),0);
        if(end==data.end() || end-(data.begin()+17)>4096) {throw Failure{ParseStatus::Malformed,"Invalid JUMBF label",current.at};}
        const auto length=static_cast<std::uint64_t>(end-(data.begin()+17));
        fields.put("box_uuid",ValueKind::Text,hex(std::span(data).first(16)),description.header,std::vector<std::uint8_t>(data.begin(),data.begin()+16),"hex");
        fields.put("box_label",ValueKind::Text,std::string(data.begin()+17,end),description.header+17,std::vector<std::uint8_t>(data.begin()+17,end),"UTF-8");
        std::uint64_t extra=18+length;
        if((data[16]&4U)!=0) {extra+=4;}if((data[16]&8U)!=0) {extra+=32;}
        if(extra>data.size()) {throw Failure{ParseStatus::Malformed,"Truncated JUMBF description options",current.at};}
        // Private salt bytes remain in the original packet and in assertion hash ranges.
        const bool known=uuid(data,"c2pa")||uuid(data,"c2ma")||uuid(data,"c2md")||uuid(data,"c2um")||uuid(data,"c2as")||uuid(data,"c2cl")||uuid(data,"c2cs")||uuid(data,"c2db")||uuid(data,"cbor")||uuid(data,"json")||uuid(data,"uuid")||uuid(data,"bfdb")||uuid(data,"bidb")||uuid(data,"c2vc")||uuid(data,"cain")||starts(data,std::string_view("\x40\xcb\x0c\x32\xbb\x8a\x48\x9d\xa7\x0b\x2a\xd6\xf4\x7f\x43\x69",16));
        if(!known) {fields.decoded("interpretation",ValueKind::Text,"unknown_UUID_contents_not_interpreted","c2pa-store"+std::to_string(store_));return;}
        for(auto at=description.size;at<body.size;) {
            auto child=box(body,at);child.at+=current.at+current.header;walk(child,id,depth+1);at+=child.size;
        }
    }
public:
    Store(Reader& packet,unsigned index):packet_(packet),store_(index){}
    bool inspect() {
        if(packet_.size<34) {throw Failure{ParseStatus::Malformed,"Truncated JUMBF store",0};}
        const auto root=box(packet_,0);if(root.type!="jumb") {return false;}
        auto body=packet_.window(root.header,root.size-root.header,packet_.group);const auto description=box(body,0);
        if(description.type!="jumd" || description.size-description.header<18) {throw Failure{ParseStatus::Malformed,"Invalid JUMBF root description",0};}
        auto data=body.read(description.header,description.size-description.header);
        if(!uuid(data,"c2pa")) {return false;}
        if(root.size!=packet_.size) {throw Failure{ParseStatus::Malformed,"Trailing bytes outside the C2PA store",root.size};}
        packet_.put("store_packet",ValueKind::Bytes,"",0,packet_.read(0,packet_.size));
        walk(root,std::numeric_limits<std::uint64_t>::max(),0);return true;
    }
};
class C2pa final:public Parser {
public:
    const ParserCapability& capability() const noexcept override {
        static const ParserCapability value{"c2pa.manifest","Bounded C2PA JUMBF manifest and assertion extraction","builtin",Probe|Extract,{"jpeg","png","c2pa"}};return value;
    }
    ParseStatus run(ParserContext& context) const override {
        Reader r(context,0,context.source_size(),"c2pa.container");unsigned stores=0;
        try {
            const auto prefix=r.read(0,std::min<std::uint64_t>(r.size,16));
            if(starts(prefix,"\xff\xd8")) {
                struct Fragments {std::uint16_t instance=0;std::uint32_t next=1;std::uint64_t end=0,total=0;std::array<std::uint8_t,8> header{};std::vector<std::uint8_t> bytes;};
                std::vector<Fragments> groups;
                for(std::uint64_t at=2;at<r.size;) {
                    r.entry();const auto start=at;auto marker=r.read(at,2);if(marker[0]!=0xff) {throw Failure{ParseStatus::Malformed,"Invalid JPEG marker before C2PA",at};}
                    while(marker[1]==0xff) {++at;marker=r.read(at,2);}
                    const auto code=marker[1];if(code==0xda || code==0xd9) {break;}
                    if(code==0x00 || code==0xd8) {throw Failure{ParseStatus::Malformed,"Unexpected JPEG marker",at};}
                    if(code==0x01 || (code>=0xd0 && code<=0xd7)) {at+=2;continue;}
                    const auto length=integer(r.read(at+2,2),0,2);if(length<2) {throw Failure{ParseStatus::Malformed,"Invalid JPEG segment length",at};}
                    r.require(at+2,length);const auto end=at+2+length;
                    if(code==0xeb && length>=18) {
                        auto header=r.read(at+4,16);
                        if(starts(header,"JP")) {
                            const auto instance=static_cast<std::uint16_t>(integer(header,2,2));const auto sequence=integer(header,4,4);
                            auto found=std::find_if(groups.begin(),groups.end(),[&](const auto& g){return g.instance==instance;});
                            if(sequence==1 && found==groups.end() && starts(header,"jumb",12)) {
                                const auto declared=integer(header,8,4);const auto extended=declared==1;
                                const auto check=r.read(at+12, std::min<std::uint64_t>(length-10,extended?48:40));
                                const auto uuid_at=extended?24U:16U;
                                if(check.size()>=uuid_at+18 && uuid(std::span(check).subspan(uuid_at),"c2pa")) {
                                    const auto total=extended?integer(check,8,8):declared;
                                    if(total<34) {throw Failure{ParseStatus::Malformed,"Invalid declared C2PA store length",at};}
                                    if(!context.budget().check_member_size(total) || !context.budget().reserve_allocation(total)) {throw Failure{ParseStatus::Internal,"C2PA store size budget",at};}
                                    Fragments group;group.instance=instance;group.total=total;std::copy(header.begin()+8,header.end(),group.header.begin());group.bytes.reserve(static_cast<std::size_t>(total));groups.push_back(std::move(group));found=groups.end()-1;
                                }
                            }
                            if(found!=groups.end()) {
                                if(sequence!=found->next || (found->next>1 && start!=found->end) || !std::equal(found->header.begin(),found->header.end(),header.begin()+8)) {throw Failure{ParseStatus::Malformed,"C2PA JPEG fragment order, contiguity or header mismatch",at};}
                                auto payload=r.read(at+4+(sequence==1?8:16),length-2-(sequence==1?8:16));
                                if(payload.size()>found->total-found->bytes.size()) {throw Failure{ParseStatus::Malformed,"C2PA fragments exceed declared store",at};}
                                r.put("jpeg_fragment",ValueKind::Bytes,"",at+4,r.read(at+4,length-2));
                                found->bytes.insert(found->bytes.end(),payload.begin(),payload.end());++found->next;found->end=end;
                                r.derived("manifest_segment_offset",start);r.derived("manifest_segment_length",end-start);
                            }
                        }
                    }
                    at=end;
                }
                for(auto& group:groups) {
                    if(group.bytes.size()!=group.total) {throw Failure{ParseStatus::Malformed,"Missing C2PA JPEG fragment bytes",0};}
                    Reader packet(context,group.bytes,"c2pa.store"+std::to_string(stores),"c2pa-store"+std::to_string(stores));
                    if(Store(packet,stores).inspect()) {++stores;}
                }
            } else if(starts(prefix,std::string_view("\x89PNG\r\n\x1a\n",8))) {
                for(std::uint64_t at=8;at<r.size;) {
                    r.entry();const auto header=r.read(at,8);const auto length=integer(header,0,4);r.require(at,12+length);const std::string type(header.begin()+4,header.end());
                    if(type=="caBX") {
                        if(!context.budget().check_member_size(length)) {throw Failure{ParseStatus::Internal,"C2PA PNG store budget",at};}
                        auto data=r.read(at+4,4+length);const auto expected=integer(r.read(at+8+length,4),0,4);
                        auto crc=crc32(0L,Z_NULL,0);for(std::size_t offset=0;offset<data.size();) {r.checkpoint();const auto size=std::min<std::size_t>(65536,data.size()-offset);crc=crc32(crc,data.data()+offset,static_cast<uInt>(size));offset+=size;}
                        if(crc!=expected) {throw Failure{ParseStatus::Malformed,"C2PA PNG CRC mismatch",at};}
                        auto packet=r.window(at+8,length,"c2pa.store"+std::to_string(stores));if(Store(packet,stores).inspect()) {++stores;r.derived("manifest_segment_offset",at);r.derived("manifest_segment_length",length+12);}
                    }
                    at+=12+length;if(type=="IEND") {break;}
                }
            } else if(prefix.size()>=8 && starts(prefix,"jumb",4)) {
                if(!context.budget().check_member_size(r.size)) {throw Failure{ParseStatus::Internal,"C2PA store size budget",0};}
                auto packet=r.window(0,r.size,"c2pa.store0");if(Store(packet,0).inspect()) {++stores;}
            } else {return ParseStatus::Unsupported;}
            r.derived("store_count",stores);r.decoded("structural_extraction_scope",ValueKind::Text,"JUMBF_structure_and_raw_packets_only; cryptographic_checks_are_separate","c2pa");
            if(stores>1) {r.diagnostic("multiple_stores","Multiple C2PA stores are ambiguous",0);return ParseStatus::Malformed;}
            return ParseStatus::Complete;
        } catch(const Failure& failure) {if(context.budget().status()==BudgetStatus::Ok) {r.diagnostic("invalid_c2pa",failure.message,std::min(failure.offset,r.size));}return failure.status;}
    }
};
}
std::unique_ptr<Parser> c2pa_manifest_parser() {return std::make_unique<C2pa>();}
}
