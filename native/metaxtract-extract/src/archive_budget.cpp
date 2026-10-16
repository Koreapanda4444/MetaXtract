#include "archive_budget.hpp"
#include <cstdlib>
#include <cstring>
#include <cwchar>
#include <limits>

namespace metaxtract::native::formats {
namespace {
thread_local ParserBudget* current = nullptr;
thread_local std::size_t emergency_remaining = 0;
}
ArchiveBudgetScope::ArchiveBudgetScope(ParserBudget& budget) noexcept
    : previous(current), previous_emergency(emergency_remaining) {
    current = &budget; emergency_remaining = emergency_bytes;
}
ArchiveBudgetScope::~ArchiveBudgetScope() { current = previous; emergency_remaining = previous_emergency; }
bool archive_charge(const std::size_t size) noexcept {
    if (!current || current->reserve_library_allocation(size)) { return true; }
    // Already charged before entering C. Error strings must remain allocatable
    // after the ordinary ceiling is exhausted; libarchive treats OOM as fatal.
    if (size > emergency_remaining) { return false; }
    emergency_remaining -= size;
    return true;
}
}
extern "C" int metaxtract_archive_decode(std::size_t count) {
    using namespace metaxtract::native::formats;
    return !current || current->consume_decoded(count);
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
