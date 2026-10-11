#include "metaxtract/native/core.hpp"

#include <bit>

namespace metaxtract::native {

CoreInfo core_info() noexcept {
    return CoreInfo{
        .api_revision = kApiRevision,
        .schema_revision = kSchemaRevision,
        .little_endian = std::endian::native == std::endian::little,
    };
}

const char* engine_name() noexcept {
    return "metaxtract-native";
}

}
