#include "metaxtract/native/core.hpp"
#include "metaxtract/native/registry.hpp"

#include <cstring>

int main() {
    const auto info = metaxtract::native::core_info();
    if (info.api_revision != metaxtract::native::kApiRevision) {
        return 1;
    }
    if (info.schema_revision != metaxtract::native::kSchemaRevision) {
        return 2;
    }
    if (std::strcmp(metaxtract::native::engine_name(), "metaxtract-native") != 0) {
        return 3;
    }
    if (!metaxtract::native::default_registry().parsers().empty()) {
        return 4;
    }
    return 0;
}
