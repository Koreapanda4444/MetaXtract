#include "format.hpp"
#include <map>
#include <optional>
#include <set>
#include "sanitized_digest.hpp"
#include <zlib.h>

namespace metaxtract::native {
namespace {
using namespace formats;
using Bytes=std::vector<std::uint8_t>;
void append(Bytes& out,std::span<const std::uint8_t> data){out.insert(out.end(),data.begin(),data.end());}
void little(Bytes& out,std::uint64_t n,unsigned width){for(unsigned i=0;i<width;++i){out.push_back(static_cast<std::uint8_t>(n>>(8U*i)));}}
void big(Bytes& out,std::uint64_t n,unsigned width){for(unsigned i=0;i<width;++i){out.push_back(static_cast<std::uint8_t>(n>>(8U*(width-i-1))));}}
void range(std::span<const std::uint8_t> data,std::uint64_t at,std::uint64_t size){if(at>data.size()||size>data.size()-at){throw Failure{ParseStatus::Malformed,"Image sanitization range escapes source",at};}}
struct RenderingField{std::uint16_t type;std::uint32_t count;Bytes data;};
class ExifRendering final {
    std::map<std::uint16_t,RenderingField> fields_;std::optional<RenderingField> color_;
    void retain(std::uint16_t tag,RenderingField field,bool color){
        auto* prior=color?(color_?&*color_:nullptr):(fields_.contains(tag)?&fields_.at(tag):nullptr);
        if(prior && (prior->type!=field.type || prior->count!=field.count || prior->data!=field.data)){throw Failure{ParseStatus::Malformed,"Conflicting EXIF rendering declarations",0};}
        if(color){color_=std::move(field);}else{fields_.insert_or_assign(tag,std::move(field));}
    }
public:
    void read(std::span<const std::uint8_t> bytes,Reader& r){
        if(starts(bytes,std::string_view("Exif\0\0",6))){bytes=bytes.subspan(6);}
        range(bytes,0,8);const bool le=starts(bytes,"II");if(!le&&!starts(bytes,"MM")){throw Failure{ParseStatus::Malformed,"Invalid EXIF byte order",0};}
        if(integer(bytes,2,2,le)!=42){throw Failure{ParseStatus::Unsupported,"Only classic EXIF TIFF is supported for sanitized images",0};}
        const auto directory=[&](std::uint64_t at,bool exif){
            range(bytes,at,2);const auto count=integer(bytes,at,2,le);range(bytes,at+2,count*12+4);std::optional<std::uint64_t> child;
            for(std::uint64_t i=0;i<count;++i){r.entry();const auto p=at+2+12*i;const auto tag=static_cast<std::uint16_t>(integer(bytes,p,2,le));const auto type=static_cast<std::uint16_t>(integer(bytes,p+2,2,le));const auto n=static_cast<std::uint32_t>(integer(bytes,p+4,4,le));
                if(!exif && tag==0x8769){if(type!=4||n!=1){throw Failure{ParseStatus::Malformed,"Invalid EXIF subdirectory",p};}const auto next=integer(bytes,p+8,4,le);if(child && *child!=next){throw Failure{ParseStatus::Malformed,"Conflicting EXIF subdirectory pointers",p};}child=next;continue;}
                unsigned wanted_type=0,wanted_count=0;
                if(exif){if(tag==0xa001){wanted_type=3;wanted_count=1;}}
                else{switch(tag){case 0x112:case 0x128:case 0x213:wanted_type=3;wanted_count=1;break;case 0x212:wanted_type=3;wanted_count=2;break;case 0x11a:case 0x11b:wanted_type=5;wanted_count=1;break;case 0x13e:wanted_type=5;wanted_count=2;break;case 0x13f:case 0x214:wanted_type=5;wanted_count=6;break;case 0x211:wanted_type=5;wanted_count=3;break;default:break;}}
                if(!wanted_type){continue;}if(type!=wanted_type||n!=wanted_count){throw Failure{ParseStatus::Malformed,"Unsupported EXIF rendering field representation",p};}
                const auto length=static_cast<std::uint64_t>(n)*(type==3?2:8);const auto offset=length<=4?p+8:integer(bytes,p+8,4,le);range(bytes,offset,length);Bytes value;value.reserve(static_cast<std::size_t>(length));
                const unsigned width=type==3?2:4;for(std::uint64_t q=0;q<length;q+=width){const auto number=integer(bytes,offset+q,width,le);if(type==5 && q%8==4 && number==0){throw Failure{ParseStatus::Malformed,"Invalid EXIF rendering rational",offset+q};}little(value,number,width);}
                if(tag==0x112 && (integer(value,0,2,true)<1 || integer(value,0,2,true)>8)){throw Failure{ParseStatus::Malformed,"Invalid EXIF orientation",p};}
                retain(tag,{type,n,std::move(value)},exif);
            }return child;
        };
        const auto root=integer(bytes,4,4,le);if(root==0){throw Failure{ParseStatus::Malformed,"Missing EXIF root directory",0};}const auto child=directory(root,false);if(child){directory(*child,true);}
    }
    Bytes packet() const {
        if(fields_.empty()&&!color_){return {};}
        const auto count=fields_.size()+(color_?1U:0U);Bytes out{'I','I',42,0,8,0,0,0};little(out,count,2);
        std::uint64_t cursor=8+2+count*12+4;Bytes data;std::uint64_t child=cursor;
        for(const auto& [tag,value]:fields_){static_cast<void>(tag);if(value.data.size()>4){child+=value.data.size();}}
        const auto entry=[&](std::uint16_t tag,const RenderingField& value){little(out,tag,2);little(out,value.type,2);little(out,value.count,4);if(value.data.size()<=4){append(out,value.data);out.insert(out.end(),4-value.data.size(),0);}else{little(out,cursor,4);append(data,value.data);cursor+=value.data.size();}};
        for(const auto& [tag,value]:fields_){entry(tag,value);}if(color_){Bytes pointer;little(pointer,child,4);entry(0x8769,{4,1,std::move(pointer)});}little(out,0,4);append(out,data);
        if(color_){little(out,1,2);entry(0xa001,*color_);little(out,0,4);}return out;
    }
};
struct Plan {std::string format;std::vector<Bytes> kept;Bytes jfif;ExifRendering exif;std::uint64_t removed=0,trailing=0;bool credentials=false;};
Bytes jpeg_segment(std::uint8_t code,std::span<const std::uint8_t> payload){if(payload.size()>65533){throw Failure{ParseStatus::Unsupported,"Sanitized JPEG metadata exceeds segment limit",0};}Bytes out{0xff,code};big(out,payload.size()+2,2);append(out,payload);return out;}
Bytes png_chunk(std::string_view type,std::span<const std::uint8_t> payload){Bytes out;big(out,payload.size(),4);append(out,std::span(reinterpret_cast<const std::uint8_t*>(type.data()),4));append(out,payload);auto crc=crc32(0L,Z_NULL,0);for(std::size_t at=4;at<out.size();){const auto size=std::min<std::size_t>(65536,out.size()-at);crc=crc32(crc,out.data()+at,static_cast<uInt>(size));at+=size;}big(out,crc,4);return out;}
Bytes webp_chunk(std::string_view type,std::span<const std::uint8_t> payload){Bytes out;append(out,std::span(reinterpret_cast<const std::uint8_t*>(type.data()),4));little(out,payload.size(),4);append(out,payload);if(payload.size()%2){out.push_back(0);}return out;}
Plan plan(std::span<const std::uint8_t> bytes,Reader& r){
    Plan out;
    if(starts(bytes,std::string_view("\xff\xd8",2))){
        out.format="jpeg";out.kept.push_back({0xff,0xd8});std::uint64_t at=2;
        while(at<bytes.size()){
            r.entry();const auto start=at;range(bytes,at,2);if(bytes[at]!=0xff || bytes[at+1]==0){throw Failure{ParseStatus::Malformed,"Invalid JPEG marker during sanitization",at};}while(bytes[at+1]==0xff){++at;range(bytes,at,2);}const auto code=bytes[at+1];at+=2;
            if(code==0xd9){out.kept.push_back({0xff,0xd9});out.trailing=bytes.size()-at;return out;}
            if(code==0xd8){throw Failure{ParseStatus::Malformed,"Repeated JPEG SOI",start};}
            if(code==1 || (code>=0xd0&&code<=0xd7)){out.kept.emplace_back(bytes.begin()+static_cast<std::ptrdiff_t>(start),bytes.begin()+static_cast<std::ptrdiff_t>(at));continue;}
            range(bytes,at,2);const auto length=integer(bytes,at,2);if(length<2){throw Failure{ParseStatus::Malformed,"Invalid JPEG segment",at};}range(bytes,at,length);const auto payload=bytes.subspan(static_cast<std::size_t>(at+2),static_cast<std::size_t>(length-2));
            if(code==0xda){
                at+=length;bool found=false;for(;at+1<bytes.size();++at){if(at%4096==0){r.checkpoint();}if(bytes[at]==0xff){auto next=at+1;while(next<bytes.size()&&bytes[next]==0xff){++next;}if(next==bytes.size()){break;}const auto marker=bytes[next];if(marker==0 || (marker>=0xd0&&marker<=0xd7)){at=next;continue;}found=true;break;}}
                if(!found){throw Failure{ParseStatus::Malformed,"JPEG entropy terminator missing",at};}out.kept.emplace_back(bytes.begin()+static_cast<std::ptrdiff_t>(start),bytes.begin()+static_cast<std::ptrdiff_t>(at));continue;
            }
            const bool application=code>=0xe0&&code<=0xef;
            if(application||code==0xfe){
                ++out.removed;
                if(code==0xe1 && starts(payload,std::string_view("Exif\0\0",6))){out.exif.read(payload,r);}
                else if(code==0xe0 && starts(payload,std::string_view("JFIF\0",5))){if(payload.size()<14){throw Failure{ParseStatus::Malformed,"Invalid JFIF header",start};}Bytes jfif(payload.begin(),payload.begin()+14);jfif[12]=0;jfif[13]=0;if(!out.jfif.empty()&&out.jfif!=jfif){throw Failure{ParseStatus::Malformed,"Conflicting JFIF rendering metadata",start};}out.jfif=std::move(jfif);}
                else if(code==0xe2 && starts(payload,std::string_view("MPF\0",4))){throw Failure{ParseStatus::Unsupported,"Multi-picture JPEG sanitization is unsupported",start};}
                else if((code==0xe2 && starts(payload,std::string_view("ICC_PROFILE\0",12)))||(code==0xee && starts(payload,"Adobe"))){if(code==0xee&&payload.size()!=12){throw Failure{ParseStatus::Unsupported,"Nonstandard Adobe color-transform segment",start};}out.kept.emplace_back(bytes.begin()+static_cast<std::ptrdiff_t>(start),bytes.begin()+static_cast<std::ptrdiff_t>(at+length));--out.removed;}
                else if(code==0xeb){out.credentials=true;}
            }else{out.kept.emplace_back(bytes.begin()+static_cast<std::ptrdiff_t>(start),bytes.begin()+static_cast<std::ptrdiff_t>(at+length));}
            at+=length;
        }throw Failure{ParseStatus::Malformed,"JPEG EOI missing",at};
    }
    if(starts(bytes,std::string_view("\x89PNG\r\n\x1a\n",8))){
        out.format="png";out.kept.emplace_back(bytes.begin(),bytes.begin()+8);std::uint64_t at=8;
        const std::set<std::string> keep{"IHDR","PLTE","IDAT","IEND","tRNS","cHRM","gAMA","iCCP","sRGB","sBIT","bKGD","pHYs","sPLT","cICP","mDCv","cLLi","acTL","fcTL","fdAT"};
        while(at<bytes.size()) {r.entry();range(bytes,at,12);const auto length=integer(bytes,at,4);range(bytes,at,12+length);const std::string type(bytes.begin()+static_cast<std::ptrdiff_t>(at+4),bytes.begin()+static_cast<std::ptrdiff_t>(at+8));
            if(type=="eXIf"){out.exif.read(bytes.subspan(static_cast<std::size_t>(at+8),static_cast<std::size_t>(length)),r);++out.removed;}
            else if(keep.contains(type)){out.kept.emplace_back(bytes.begin()+static_cast<std::ptrdiff_t>(at),bytes.begin()+static_cast<std::ptrdiff_t>(at+12+length));}
            else{if((bytes[at+4]&32U)==0){throw Failure{ParseStatus::Unsupported,"Unknown critical PNG chunk cannot be sanitized safely",at};}++out.removed;if(type=="caBX"){out.credentials=true;}}
            at+=12+length;if(type=="IEND"){out.trailing=bytes.size()-at;return out;}
        }throw Failure{ParseStatus::Malformed,"PNG IEND missing",at};
    }
    if(starts(bytes,"RIFF")&&starts(bytes,"WEBP",8)){
        range(bytes,0,12);out.format="webp";const auto extent=integer(bytes,4,4,true)+8;range(bytes,0,extent);out.trailing=bytes.size()-extent;std::uint64_t at=12;
        const std::set<std::string> keep{"VP8 ","VP8L","VP8X","ALPH","ANIM","ANMF","ICCP"};
        while(at<extent){r.entry();range(bytes,at,8);const auto length=integer(bytes,at+4,4,true);range(bytes,at,8+length+length%2);if(8+length+length%2>extent-at){throw Failure{ParseStatus::Malformed,"WebP chunk escapes RIFF",at};}const std::string type(bytes.begin()+static_cast<std::ptrdiff_t>(at),bytes.begin()+static_cast<std::ptrdiff_t>(at+4));const auto payload=bytes.subspan(static_cast<std::size_t>(at+8),static_cast<std::size_t>(length));
            if(type=="EXIF"){out.exif.read(payload,r);++out.removed;}else if(keep.contains(type)){auto chunk=webp_chunk(type,payload);if(type=="VP8X"){if(length!=10){throw Failure{ParseStatus::Malformed,"Invalid VP8X",at};}chunk[8]&=static_cast<std::uint8_t>(~0x0cU);}out.kept.push_back(std::move(chunk));}else{++out.removed;if(type=="C2PA"||type=="JUMB"){out.credentials=true;}}
            at+=8+length+length%2;
        }return out;
    }
    throw Failure{ParseStatus::Unsupported,"Sanitized image copies support JPEG, PNG and WebP only",0};
}
std::string digest(const Plan& plan,Reader& r){
    if(!r.context.budget().reserve_allocation(4096)){throw Failure{ParseStatus::Internal,"Image digest budget",0};}SanitizedSha256 hash;
    for(const auto& bytes:plan.kept){for(std::size_t at=0;at<bytes.size();){r.checkpoint();const auto n=std::min<std::size_t>(65536,bytes.size()-at);hash.update(bytes.data()+at,n);at+=n;}}
    const auto exif=plan.exif.packet();if(!exif.empty()){hash.update(exif.data(),exif.size());}if(!plan.jfif.empty()){hash.update(plan.jfif.data(),plan.jfif.size());}return hash.finish();
}
Bytes output(const Plan& plan){
    Bytes out;auto exif=plan.exif.packet();
    if(plan.format=="jpeg") {append(out,plan.kept.front());if(!plan.jfif.empty()){append(out,jpeg_segment(0xe0,plan.jfif));}if(!exif.empty()){Bytes packet{'E','x','i','f',0,0};append(packet,exif);append(out,jpeg_segment(0xe1,packet));}for(std::size_t i=1;i<plan.kept.size();++i){append(out,plan.kept[i]);}}
    else if(plan.format=="png") {for(std::size_t i=0;i<plan.kept.size();++i){append(out,plan.kept[i]);if(i==1&&!exif.empty()){append(out,png_chunk("eXIf",exif));}}}
    else {out={'R','I','F','F',0,0,0,0,'W','E','B','P'};bool extended=false;for(const auto& bytes:plan.kept){auto copy=bytes;if(starts(copy,"VP8X")){extended=true;if(!exif.empty()){copy[8]|=8U;}}append(out,copy);}if(!exif.empty()){if(!extended){throw Failure{ParseStatus::Unsupported,"EXIF in a simple WebP without VP8X is unsupported",0};}append(out,webp_chunk("EXIF",exif));}if(out.size()-8>UINT32_MAX){throw Failure{ParseStatus::Unsupported,"WebP output size exceeds RIFF",0};}for(unsigned i=0;i<4;++i){out[4+i]=static_cast<std::uint8_t>((out.size()-8)>>(8U*i));}}
    return out;
}
void validate(Reader& r,const std::string& format){if(format=="jpeg"){inspect_jpeg(r,false);}else if(format=="png"){inspect_png(r,false);}else{inspect_webp(r,false);}}
class SanitizeImage final:public Parser {
public:
    const ParserCapability& capability() const noexcept override {static const ParserCapability value{"images.sanitize","In-memory sanitized JPEG PNG WebP copies preserving rendering data","builtin",Extract,{"jpeg","png","webp"}};return value;}
    ParseStatus run(ParserContext& context) const override {
        Reader r(context,0,context.source_size(),"sanitize.image");
        try {
            if(!context.budget().check_member_size(r.size)||!context.budget().reserve_allocation((r.size+65536)*4)){throw Failure{ParseStatus::Internal,"Sanitized image working budget",0};}
            const auto bytes=r.read(0,r.size);Reader input(context,bytes,"sanitize.image.input","original-captured");const auto planned=plan(bytes,input);validate(input,planned.format);const auto before=digest(planned,input);const auto transformed=output(planned);
            Reader after(context,transformed,"sanitize.image.output","sanitized-copy");validate(after,planned.format);const auto checked=plan(transformed,after);const auto after_digest=digest(checked,after);
            if(before!=after_digest){throw Failure{ParseStatus::Internal,"Sanitization changed protected rendering data",0};}
            r.decoded("format",ValueKind::Text,planned.format,"sanitized-copy");r.derived("removed_metadata_blocks",planned.removed);r.derived("removed_trailing_bytes",planned.trailing);r.derived("authenticity_metadata_removed",planned.credentials?1:0);
            r.decoded("rendering_before_sha256",ValueKind::Text,before,"sanitized-copy");r.decoded("rendering_after_sha256",ValueKind::Text,after_digest,"sanitized-copy");
            r.decoded("policy",ValueKind::Text,"remove_nonrendering_metadata_and_previews; preserve_encoded_image_data_orientation_resolution_color_profiles_and_animation; visual_content_and_retained_profiles_are_not_anonymized","sanitized-copy");
            r.decoded("sanitized_payload",ValueKind::DecodedBytes,std::string(transformed.begin(),transformed.end()),"sanitized-copy");return ParseStatus::Complete;
        }catch(const Failure& failure){if(context.budget().status()==BudgetStatus::Ok){r.diagnostic("sanitize_failed",failure.message,std::min(failure.offset,r.size));}return failure.status;}
    }
};
}
std::unique_ptr<Parser> image_sanitization_parser(){return std::make_unique<SanitizeImage>();}
}
