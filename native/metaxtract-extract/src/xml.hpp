#pragma once

#include "metaxtract/native/budget.hpp"
#include <pugixml.hpp>
#include <string>
#include <string_view>

namespace metaxtract::native::formats {

class XmlBudgetScope final {
    ParserBudget* previous_;
public:
    explicit XmlBudgetScope(ParserBudget& budget);
    ~XmlBudgetScope();
    XmlBudgetScope(const XmlBudgetScope&) = delete;
    XmlBudgetScope& operator=(const XmlBudgetScope&) = delete;
};

std::string expanded_xml_name(pugi::xml_node node, const std::string& name, bool attribute = false);
bool valid_xml_string(std::string_view value);

}
