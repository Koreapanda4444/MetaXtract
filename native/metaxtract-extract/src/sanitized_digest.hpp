#pragma once
#include <qpdf/QPDFCryptoProvider.hh>
#include <algorithm>
#include <array>
#include <cstring>
#include <cstdint>
#include <string>

namespace metaxtract::native {
// QPDF's native SHA implementation may optimize full blocks using aligned
// word loads. Feed aligned complete blocks, keeping arbitrary caller offsets
// and short fragments in our own fixed-size buffer. No checks are disabled.
class SanitizedSha256 final {
    decltype(QPDFCryptoProvider::getImpl()) hash_ = QPDFCryptoProvider::getImpl();
    std::array<std::uint64_t, 8> block_{};
    std::size_t used_ = 0;
public:
    SanitizedSha256() { hash_->SHA2_init(256); }
    void update(const unsigned char* data, std::size_t size) {
        auto* block = reinterpret_cast<unsigned char*>(block_.data());
        constexpr auto capacity = sizeof(block_);
        while (size != 0) {
            const auto count = std::min(size, capacity - used_);
            std::memcpy(block + used_, data, count);
            data += count; size -= count; used_ += count;
            if (used_ == capacity) {
                hash_->SHA2_update(block, capacity);
                used_ = 0;
            }
        }
    }
    std::string finish() {
        if (used_ != 0) { hash_->SHA2_update(reinterpret_cast<const unsigned char*>(block_.data()), used_); }
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
