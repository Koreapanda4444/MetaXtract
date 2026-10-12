#pragma once

#include <stdlib.h>
#include <string.h>
#include <wchar.h>

#ifdef __cplusplus
extern "C" {
#endif
void* metaxtract_archive_malloc(size_t);
void* metaxtract_archive_calloc(size_t, size_t);
void* metaxtract_archive_realloc(void*, size_t);
char* metaxtract_archive_strdup(const char*);
wchar_t* metaxtract_archive_wcsdup(const wchar_t*);
#ifdef __cplusplus
}
#endif

#define malloc metaxtract_archive_malloc
#define calloc metaxtract_archive_calloc
#define realloc metaxtract_archive_realloc
#ifdef strdup
#undef strdup
#endif
#define strdup metaxtract_archive_strdup
#define _strdup metaxtract_archive_strdup
#define _wcsdup metaxtract_archive_wcsdup
