#include "zip.hpp"
#include "xml.hpp"
#include "pdf_budget.hpp"
#include <qpdf/Buffer.hh>
#include <qpdf/QPDF.hh>
#include <qpdf/QPDFWriter.hh>
#include <qpdf/QPDFCryptoProvider.hh>
#include <map>
#include <set>
#include <sstream>
#include <zlib.h>

namespace metaxtract::native {
namespace {
using namespace formats;
using Bytes = std::vector<std::uint8_t>;
constexpr std::uint32_t author = 1, title = 2, description = 4, dates = 8, custom = 16, application = 32;
struct Digest {
    decltype(QPDFCryptoProvider::getImpl()) hash = QPDFCryptoProvider::getImpl();
    explicit Digest() { hash->SHA2_init(256); }
    void bytes(std::span<const std::uint8_t> data, Reader& r) {
        std::uint8_t length[8]; for (unsigned i=0; i<8; ++i) { length[i]=static_cast<std::uint8_t>(data.size()>>(i*8U)); }
        hash->SHA2_update(length,8);
        for (std::size_t at=0; at<data.size();) { r.checkpoint(); const auto n=std::min<std::size_t>(65536,data.size()-at); hash->SHA2_update(data.data()+at,n); at+=n; }
    }
    void text(const std::string& value, Reader& r) { bytes(std::span(reinterpret_cast<const std::uint8_t*>(value.data()),value.size()),r); }
    std::string finish() { hash->SHA2_finalize(); constexpr char hex[]="0123456789abcdef"; std::string out; for (const unsigned char b:hash->SHA2_digest()) { out.push_back(hex[b>>4U]); out.push_back(hex[b&15U]); } return out; }
};
std::string local(std::string name) { const auto p=name.rfind('}'); if(p!=std::string::npos){return name.substr(p+1);} const auto c=name.find(':');return name.substr(c==std::string::npos?0:c+1); }
std::uint32_t group(const std::string& expanded) {
    const auto name=local(expanded);
    if (name=="creator" || name=="Author" || name=="lastModifiedBy" || name=="initial-creator" || name=="printed-by" || name=="Manager" || name=="Company") { return author; }
    if (name=="title" || name=="Title") { return title; }
    if (name=="description" || name=="Description" || name=="subject" || name=="Subject" || name=="Keywords" || name=="keywords" || name=="keyword" || name=="category" || name=="contentStatus" || name=="rights") { return description; }
    if (name=="created" || name=="modified" || name=="lastPrinted" || name=="date" || name=="creation-date" || name=="print-date" || name=="editing-duration" || name=="editing-cycles" || name=="CreateDate" || name=="ModifyDate" || name=="MetadataDate") { return dates; }
    if (name=="generator" || name=="Application" || name=="AppVersion" || name=="CreatorTool" || name=="Producer" || name=="Template" || name=="template") { return application; }
    return custom;
}
Bytes xml_properties(Reader& r, const Bytes& bytes, const std::string& kind, std::uint32_t mask, std::uint64_t& removed) {
    XmlBudgetScope scope(r.context.budget()); pugi::xml_document doc;
    if (!doc.load_buffer(bytes.data(),bytes.size(),pugi::parse_default|pugi::parse_comments|pugi::parse_pi|pugi::parse_doctype) || !doc.document_element()) { throw Failure{ParseStatus::Malformed,"Invalid metadata property XML",0}; }
    const auto root=expanded_xml_name(doc.document_element(),doc.document_element().name());
    const auto expected=[&](std::string_view uri,std::string_view name){return root=="{"+std::string(uri)+"}"+std::string(name);};
    const bool valid=kind=="xmp"?(expected("adobe:ns:meta/","xmpmeta")||expected("adobe:ns:meta/","xapmeta")||expected("http://www.w3.org/1999/02/22-rdf-syntax-ns#","RDF")):
        kind=="odf"?expected("urn:oasis:names:tc:opendocument:xmlns:office:1.0","document-meta"):
        kind=="core"?(expected("http://schemas.openxmlformats.org/package/2006/metadata/core-properties","coreProperties")||expected("http://purl.oclc.org/ooxml/package/metadata/core-properties","coreProperties")):
        kind=="app"?(expected("http://schemas.openxmlformats.org/officeDocument/2006/extended-properties","Properties")||expected("http://purl.oclc.org/ooxml/officeDocument/extendedProperties","Properties")):
        kind=="custom"?(expected("http://schemas.openxmlformats.org/officeDocument/2006/custom-properties","Properties")||expected("http://purl.oclc.org/ooxml/officeDocument/customProperties","Properties")):false;
    if(!valid){throw Failure{ParseStatus::Unsupported,"Unexpected document property XML root namespace",0};}
    auto node=doc.first_child(); std::uint32_t depth=1;
    while(node) {
        r.entry();r.checkpoint(); if(!r.context.budget().check_depth(depth) || depth>64){throw Failure{ParseStatus::Internal,"Metadata XML depth budget",0};}
        if(node.type()==pugi::node_doctype){throw Failure{ParseStatus::Unsupported,"DTD metadata cannot be sanitized safely",0};}
        if(node.first_child()){node=node.first_child();++depth;}else{while(node&&!node.next_sibling()){node=node.parent();if(depth>1){--depth;}}if(node){node=node.next_sibling();}}
    }
    const auto clean=[&](auto&& self,pugi::xml_node parent,std::uint32_t level)->void {
        r.checkpoint(); if(level>64){throw Failure{ParseStatus::Internal,"Metadata XML depth budget",0};}
        for(auto child=parent.first_child();child;){
            const auto next=child.next_sibling();r.entry();
            const auto parent_name=expanded_xml_name(parent,parent.name());
            const bool property=(kind=="xmp" && parent_name=="{http://www.w3.org/1999/02/22-rdf-syntax-ns#}Description") ||
                (kind=="odf" && parent_name=="{urn:oasis:names:tc:opendocument:xmlns:office:1.0}meta") ||
                ((kind=="core"||kind=="app"||kind=="custom") && parent==doc.document_element());
            const auto selected=kind=="app"?application:kind=="custom"?custom:group(expanded_xml_name(child,child.name()));
            if(child.type()==pugi::node_comment || child.type()==pugi::node_pi || (property && child.type()==pugi::node_element && (mask&selected)!=0)){
                parent.remove_child(child);++removed;
            }else{self(self,child,level+1);}
            child=next;
        }
        if(kind=="xmp" && expanded_xml_name(parent,parent.name())=="{http://www.w3.org/1999/02/22-rdf-syntax-ns#}Description"){
            for(auto attr=parent.first_attribute();attr;){const auto next=attr.next_attribute();const std::string name=attr.name();
                const auto expanded=expanded_xml_name(parent,name,true);
                if(name!="xmlns"&&!name.starts_with("xmlns:")&&!expanded.starts_with("{http://www.w3.org/1999/02/22-rdf-syntax-ns#}") && (mask&group(expanded))!=0){parent.remove_attribute(attr);++removed;}attr=next;
            }
        }
    }; clean(clean,doc,0);
    std::ostringstream out;doc.save(out,"",pugi::format_raw,pugi::encoding_utf8);const auto value=out.str();
    if(!r.context.budget().reserve_allocation(value.size()*2)){throw Failure{ParseStatus::Internal,"Metadata XML output budget",0};}
    return Bytes(value.begin(),value.end());
}
void pdf_node(Digest& hash, Reader& r, QPDFObjectHandle node, std::map<QPDFObjGen,std::uint64_t>& seen, std::uint32_t depth, bool root=false) {
    r.entry();r.checkpoint();if(depth>64||!r.context.budget().check_depth(depth)){throw Failure{ParseStatus::Internal,"Protected PDF graph depth budget",0};}
    if(node.isIndirect()){
        if(!r.context.budget().reserve_allocation(128)){throw Failure{ParseStatus::Internal,"Protected PDF graph budget",0};}
        const auto [at,inserted]=seen.emplace(node.getObjGen(),seen.size());hash.text("reference:"+std::to_string(at->second),r);if(!inserted){return;}
    }
    hash.text(node.getTypeName(),r);
    if(node.isArray()){for(int i=0;i<node.getArrayNItems();++i){pdf_node(hash,r,node.getArrayItem(i),seen,depth+1);}}
    else if(node.isDictionary()||node.isStream()){
        auto dict=node.isStream()?node.getDict():node;
        for(const auto& key:dict.getKeys()){if((root&&key=="/Metadata")||(node.isStream()&&key=="/Length")){continue;}hash.text(key,r);pdf_node(hash,r,dict.getKey(key),seen,depth+1);}
        if(node.isStream()){const auto data=node.getRawStreamData();hash.bytes(std::span(data->getBuffer(),data->getSize()),r);}
    }else if(node.isString()){hash.text(node.getStringValue(),r);}else{hash.text(node.unparse(),r);}
    hash.text("end",r);
}
std::string pdf_digest(Reader& r,QPDF& pdf){Digest hash;std::map<QPDFObjGen,std::uint64_t> seen;pdf_node(hash,r,pdf.getRoot(),seen,0,true);return hash.finish();}
std::uint32_t info_group(const std::string& key){if(key=="/Author"){return author;}if(key=="/Title"){return title;}if(key=="/Subject"||key=="/Keywords"){return description;}if(key=="/CreationDate"||key=="/ModDate"){return dates;}if(key=="/Creator"||key=="/Producer"){return application;}return custom;}
struct Result{Bytes bytes;std::string format,before,after;std::uint64_t removed=0;bool signatures=false;};
Result pdf_copy(Reader& r,const Bytes& input,std::uint32_t mask) {
    pdf::BufferBudgetScope scope(r.context.budget());QPDF document;document.setSuppressWarnings(true);document.setAttemptRecovery(false);
    document.processMemoryFile("sanitized PDF input",reinterpret_cast<const char*>(input.data()),input.size());
    if(document.isEncrypted()){throw Failure{ParseStatus::Unsupported,"Encrypted PDF sanitized copies are unsupported; original remains protected",0};}
    Result result;result.format="pdf";result.before=pdf_digest(r,document);
    for(auto object:document.getAllObjects()){r.entry();r.checkpoint();if(object.isDictionary()&&(object.hasKey("/ByteRange")||(object.getKey("/Type").isName()&&object.getKey("/Type").getName()=="/Sig"))){result.signatures=true;}}
    auto trailer=document.getTrailer();auto info=trailer.getKey("/Info");
    if(!info.isNull()&&!info.isDictionary()){throw Failure{ParseStatus::Malformed,"PDF Info is not a dictionary",0};}
    if(info.isDictionary()){for(const auto& key:info.getKeys()){if((mask&info_group(key))!=0){info.removeKey(key);++result.removed;}}if(info.getKeys().empty()){trailer.removeKey("/Info");}}
    trailer.removeKey("/ID");auto root=document.getRoot();auto metadata=root.getKey("/Metadata");
    if(!metadata.isNull()){
        if(!metadata.isStream()){throw Failure{ParseStatus::Malformed,"PDF catalog metadata is not a stream",0};}
        if(mask==63){root.removeKey("/Metadata");++result.removed;}else{
            auto data=metadata.getStreamData(qpdf_dl_generalized);Bytes bytes(data->getBuffer(),data->getBuffer()+data->getSize());const auto clean=xml_properties(r,bytes,"xmp",mask,result.removed);
            metadata.replaceStreamData(std::string(clean.begin(),clean.end()),QPDFObjectHandle::newNull(),QPDFObjectHandle::newNull());
        }
    }
    QPDFWriter writer(document);writer.setOutputMemory();writer.setPreserveUnreferencedObjects(false);writer.setStreamDataMode(qpdf_s_preserve);writer.setObjectStreamMode(qpdf_o_disable);writer.setDeterministicID(true);writer.write();
    const auto buffer=writer.getBufferSharedPointer();if(!r.context.budget().check_member_size(buffer->getSize())){throw Failure{ParseStatus::Internal,"PDF sanitized output member budget",0};}
    result.bytes.assign(buffer->getBuffer(),buffer->getBuffer()+buffer->getSize());QPDF checked;checked.setSuppressWarnings(true);checked.setAttemptRecovery(false);
    checked.processMemoryFile("sanitized PDF verification",reinterpret_cast<const char*>(result.bytes.data()),result.bytes.size());result.after=pdf_digest(r,checked);return result;
}
void little(Bytes& out,std::uint64_t n,unsigned width){for(unsigned i=0;i<width;++i){out.push_back(static_cast<std::uint8_t>(n>>(8U*i)));}}
void append(Bytes& out,std::span<const std::uint8_t> bytes){out.insert(out.end(),bytes.begin(),bytes.end());}
std::string part_kind(const std::string& name,bool ooxml){if(ooxml&&name=="meta.xml"){return {};}if(!ooxml&&name.starts_with("docProps/")){return {};}if(name=="docProps/core.xml"){return "core";}if(name=="docProps/app.xml"){return "app";}if(name=="docProps/custom.xml"){return "custom";}if(name=="meta.xml"){return "odf";}return {};}
struct Part{std::string name;Bytes bytes;};
std::string package_digest(Reader& r,const std::vector<Part>& parts,bool ooxml){Digest hash;for(const auto& part:parts){if(part_kind(part.name,ooxml).empty()){hash.text(part.name,r);hash.bytes(part.bytes,r);}}return hash.finish();}
Bytes zip_write(Reader& r,const std::vector<Part>& parts){
    if(parts.size()>65535){throw Failure{ParseStatus::Unsupported,"ZIP64 sanitized packages are unsupported",0};}
    Bytes out,central;for(const auto& part:parts){
        r.entry();r.checkpoint();if(part.name.size()>65535||part.bytes.size()>UINT32_MAX||out.size()>UINT32_MAX){throw Failure{ParseStatus::Unsupported,"Sanitized ZIP32 bounds",0};}
        if(!r.context.budget().reserve_allocation(part.bytes.size()*2+part.name.size()*4+256)){throw Failure{ParseStatus::Internal,"Package writer budget",0};}
        auto crc=crc32(0L,Z_NULL,0);for(std::size_t at=0;at<part.bytes.size();){r.checkpoint();const auto n=std::min<std::size_t>(65536,part.bytes.size()-at);crc=crc32(crc,part.bytes.data()+at,static_cast<uInt>(n));at+=n;}
        const auto offset=out.size();little(out,0x04034b50,4);little(out,20,2);little(out,0x800,2);little(out,0,2);little(out,0,2);little(out,33,2);little(out,crc,4);little(out,part.bytes.size(),4);little(out,part.bytes.size(),4);little(out,part.name.size(),2);little(out,0,2);append(out,std::span(reinterpret_cast<const std::uint8_t*>(part.name.data()),part.name.size()));append(out,part.bytes);
        little(central,0x02014b50,4);little(central,20,2);little(central,20,2);little(central,0x800,2);little(central,0,2);little(central,0,2);little(central,33,2);little(central,crc,4);little(central,part.bytes.size(),4);little(central,part.bytes.size(),4);little(central,part.name.size(),2);little(central,0,2);little(central,0,2);little(central,0,2);little(central,0,2);little(central,0,4);little(central,offset,4);append(central,std::span(reinterpret_cast<const std::uint8_t*>(part.name.data()),part.name.size()));
    }
    const auto at=out.size();append(out,central);little(out,0x06054b50,4);little(out,0,2);little(out,0,2);little(out,parts.size(),2);little(out,parts.size(),2);little(out,central.size(),4);little(out,at,4);little(out,0,2);return out;
}
Result package_copy(Reader& r,std::uint32_t mask){
    const auto entries=zip_directory(r);std::set<std::string> seen;std::vector<Part> parts;bool ooxml=false,odf=false;Result result;
    for(const auto& entry:entries){
        r.entry();r.checkpoint();if(!seen.insert(entry.name).second||unsafe_member_name(entry.name)||!valid_xml_string(entry.name)){throw Failure{ParseStatus::Malformed,"Ambiguous or invalid package member",entry.central_offset};}
        auto bytes=zip_payload(r,entry);
        if(entry.name=="[Content_Types].xml"){ooxml=true;}
        if(entry.name=="mimetype"){const std::string type(bytes.begin(),bytes.end());odf=type.starts_with("application/vnd.oasis.opendocument.");if(odf&&(entry.index!=0||entry.compression!=0)){throw Failure{ParseStatus::Malformed,"ODF mimetype must be the first stored member",entry.local_offset};}}
        if(entry.name.starts_with("_xmlsignatures/")||entry.name=="META-INF/documentsignatures.xml"||entry.name=="META-INF/macrosignatures.xml"){result.signatures=true;}
        if(entry.name=="META-INF/manifest.xml"&&std::string(bytes.begin(),bytes.end()).find("encryption-data")!=std::string::npos){throw Failure{ParseStatus::Unsupported,"Encrypted ODF sanitized copies are unsupported",0};}
        parts.push_back({entry.name,std::move(bytes)});
    }
    if(ooxml==odf){throw Failure{ParseStatus::Unsupported,"Sanitized package must have one OOXML or ODF family",0};}
    result.format=ooxml?"ooxml":"odf";result.before=package_digest(r,parts,ooxml);
    for(auto& part:parts){const auto kind=part_kind(part.name,ooxml);if(!kind.empty()){part.bytes=xml_properties(r,part.bytes,kind,mask,result.removed);}}
    result.bytes=zip_write(r,parts);if(!r.context.budget().check_member_size(result.bytes.size())){throw Failure{ParseStatus::Internal,"Sanitized package member budget",0};}
    Reader output(r.context,result.bytes,"sanitize.document.output","sanitized-copy");std::vector<Part> verified;
    for(const auto& entry:zip_directory(output)){verified.push_back({entry.name,zip_payload(output,entry)});}
    result.after=package_digest(output,verified,ooxml);return result;
}
class SanitizeDocument final:public Parser {
public:
    const ParserCapability& capability() const noexcept override {static const ParserCapability value{"documents.sanitize","In-memory selected PDF OOXML ODF property removal","builtin",Extract,{"pdf","ooxml","odf"}};return value;}
    ParseStatus run(ParserContext& context) const override {
        Reader r(context,0,context.source_size(),"sanitize.document");
        try {
            const auto mask=context.sanitize_mask();if(mask==0||mask>63){throw Failure{ParseStatus::InvalidRequest,"Select at least one supported property group",0};}
            if(!context.budget().check_member_size(r.size)||!context.budget().reserve_allocation((r.size+65536)*6+32U*1024U*1024U)){throw Failure{ParseStatus::Internal,"Sanitized document working budget",0};}
            const auto bytes=r.read(0,r.size);Result result;
            if(starts(bytes,"%PDF-")){result=pdf_copy(r,bytes,mask);}else if(starts(bytes,"PK")){Reader captured(context,bytes,"sanitize.document.input","original-captured");result=package_copy(captured,mask);}else{throw Failure{ParseStatus::Unsupported,"Sanitized document copies support PDF OOXML ODF only",0};}
            if(result.before!=result.after){throw Failure{ParseStatus::Internal,"Sanitization changed protected document content",0};}
            r.decoded("format",ValueKind::Text,result.format,"sanitized-copy");r.derived("removed_metadata_blocks",result.removed);r.derived("removed_trailing_bytes",0);r.derived("authenticity_metadata_removed",result.signatures?1:0);
            r.decoded("rendering_before_sha256",ValueKind::Text,result.before,"sanitized-copy");r.decoded("rendering_after_sha256",ValueKind::Text,result.after,"sanitized-copy");
            r.decoded("policy",ValueKind::Text,"selected_document_properties_only_mask_"+std::to_string(mask)+"; PDF_root_graph_excluding_catalog_metadata_and_Info_or_package_nonproperty_payloads_preserved; body_comments_revisions_macros_attachments_and_signatures_may_remain","sanitized-copy");
            r.decoded("sanitized_payload",ValueKind::DecodedBytes,std::string(result.bytes.begin(),result.bytes.end()),"sanitized-copy");return ParseStatus::Complete;
        }catch(const Failure& failure){if(context.budget().status()==BudgetStatus::Ok){r.diagnostic("sanitize_failed",failure.message,std::min(failure.offset,r.size));}return failure.status;}
        catch(const std::exception&){r.diagnostic("sanitize_failed","Document library rejected sanitized copy; no output published",0);return ParseStatus::Malformed;}
    }
};
}
std::unique_ptr<Parser> document_sanitization_parser(){return std::make_unique<SanitizeDocument>();}
}
