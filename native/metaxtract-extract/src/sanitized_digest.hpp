#pragma once
#include <qpdf/QPDFCryptoProvider.hh>
#include <algorithm>
#include <array>
#include <cstring>
#include <string>

namespace metaxtract::native {
// QPDF's native SHA implementation may optimize full blocks using aligned
// word loads. Feed aligned complete blocks, keeping arbitrary caller offsets
// and short fragments in our own fixed-size buffer. No checks are disabled.
class SanitizedSha256 final {
    decltype(QPDFCryptoProvider::getImpl()) hash_ = QPDFCryptoProvider::getImpl();
    alignas(64) std::array<unsigned char, 64> block_{};
    std::size_t used_ = 0;
public:
    SanitizedSha256() { hash_->SHA2_init(256); }
    void update(const unsigned char* data, std::size_t size) {
        while (size != 0) {
            const auto count = std::min(size, block_.size() - used_);
            std::memcpy(block_.data() + used_, data, count);
            data += count; size -= count; used_ += count;
            if (used_ == block_.size()) {
                hash_->SHA2_update(block_.data(), block_.size());
                used_ = 0;
            }
        }
    }
    std::string finish() {
        if (used_ != 0) { hash_->SHA2_update(block_.data(), used_); }
        hash_->SHA2_finalize();
        constexpr char hex[] = "0123456789abcdef";
        std::string out;
        for (const unsigned char byte : hash_->SHA2_digest()) {
            out.push_back(hex[byte >> 4U]); out.push_back(hex[byte & 15U]);
        }
        return out;
    }
};
}
