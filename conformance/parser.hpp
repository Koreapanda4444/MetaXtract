#pragma once

#include "metaxtract/native/parser.hpp"

#include <chrono>
#include <stdexcept>
#include <thread>

namespace metaxtract::native {

class ConformanceParser final : public Parser {
public:
    const ParserCapability& capability() const noexcept override {
        static const ParserCapability value{
            .id = "conformance.resource_limits",
            .name = "Conformance fixture parser",
            .version = "fixture",
            .operations = ParserOperation::Extract,
            .formats = {},
        };
        return value;
    }

    ParseStatus run(ParserContext& context) const override {
        const auto header = context.read_at(0, 1);
        if (header.status != ReadStatus::Ok || header.bytes.empty()) {
            return ParseStatus::ReadFailure;
        }
        auto& budget = context.budget();
        switch (header.bytes.front()) {
        case 0: {
            const auto payload = context.read_at(1, 3);
            return payload.status == ReadStatus::Ok ? ParseStatus::Complete : ParseStatus::ReadFailure;
        }
        case 1:
            static_cast<void>(context.read_at(0, 4));
            break;
        case 2:
            static_cast<void>(budget.reserve_allocation(16));
            break;
        case 3:
            static_cast<void>(budget.consume_decoded(4));
            static_cast<void>(budget.consume_decoded(1));
            break;
        case 4:
            static_cast<void>(budget.check_depth(3));
            break;
        case 5:
            static_cast<void>(budget.add_entries(3));
            break;
        case 6:
            static_cast<void>(budget.add_observations(3));
            break;
        case 7:
            throw std::runtime_error("conformance parser exception");
        case 8:
            std::this_thread::sleep_for(std::chrono::milliseconds(5));
            break;
        default:
            return ParseStatus::InvalidRequest;
        }
        return ParseStatus::Complete;
    }
};

}
