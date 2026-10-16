#include "archive_budget.hpp"
#include <cstdlib>
#include <cstring>
#include <cwchar>
#include <limits>

namespace metaxtract::native::formats {
namespace { thread_local ParserBudget* current = nullptr; }
ArchiveBudgetScope::ArchiveBudgetScope(ParserBudget& budget) noexcept : previous(current) { current = &budget; }
ArchiveBudgetScope::~ArchiveBudgetScope() { current = previous; }
bool archive_charge(const std::size_t size) noexcept {
    return !current || current->reserve_library_allocation(size);
}
}
extern "C" void* metaxtract_archive_malloc(std::size_t count) {
    return metaxtract::native::formats::archive_charge(count) ? std::malloc(count) : nullptr;
}
extern "C" void* metaxtract_archive_calloc(std::size_t count, std::size_t size) {
    if (size != 0 && count > std::numeric_limits<std::size_t>::max() / size) {
        static_cast<void>(metaxtract::native::formats::archive_charge(std::numeric_limits<std::size_t>::max()));
        return nullptr;
    }
    return metaxtract::native::formats::archive_charge(count * size) ? std::calloc(count, size) : nullptr;
}
extern "C" void* metaxtract_archive_realloc(void* pointer, std::size_t count) {
    return metaxtract::native::formats::archive_charge(count) ? std::realloc(pointer, count) : nullptr;
}
extern "C" char* metaxtract_archive_strdup(const char* text) {
    const auto count = std::strlen(text) + 1;
    auto* data = static_cast<char*>(metaxtract_archive_malloc(count));
    if (data) { std::memcpy(data, text, count); }
    return data;
}
extern "C" wchar_t* metaxtract_archive_wcsdup(const wchar_t* text) {
    const auto count = std::wcslen(text) + 1;
    if (count > std::numeric_limits<std::size_t>::max() / sizeof(wchar_t)) { return nullptr; }
    auto* data = static_cast<wchar_t*>(metaxtract_archive_malloc(count * sizeof(wchar_t)));
    if (data) { std::memcpy(data, text, count * sizeof(wchar_t)); }
    return data;
}
