#pragma once

#include <cstdint>

namespace metaxtract::native {

inline constexpr std::uint32_t kApiRevision = 1;
inline constexpr std::uint32_t kSchemaRevision = 1;

struct CoreInfo final {
    std::uint32_t api_revision;
    std::uint32_t schema_revision;
    bool little_endian;
};

[[nodiscard]] CoreInfo core_info() noexcept;
[[nodiscard]] const char* engine_name() noexcept;

}
