#include "parser.hpp"

#include <memory>

namespace metaxtract::native {

std::unique_ptr<Parser> conformance_parser() {
    return std::make_unique<ConformanceParser>();
}

}
