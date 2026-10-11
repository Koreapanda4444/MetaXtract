option(METAXTRACT_WARNINGS_AS_ERRORS "Treat native compiler warnings as errors" OFF)

add_library(metaxtract_extract STATIC
  src/core.cpp
)

target_include_directories(metaxtract_extract
  PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/include
)
target_compile_features(metaxtract_extract PUBLIC cxx_std_20)

if(MSVC)
  target_compile_options(metaxtract_extract PRIVATE /W4 /permissive-)
  if(METAXTRACT_WARNINGS_AS_ERRORS)
    target_compile_options(metaxtract_extract PRIVATE /WX)
  endif()
else()
  target_compile_options(metaxtract_extract PRIVATE -Wall -Wextra -Wpedantic)
  if(METAXTRACT_WARNINGS_AS_ERRORS)
    target_compile_options(metaxtract_extract PRIVATE -Werror)
  endif()
endif()

if(BUILD_TESTING)
  add_executable(metaxtract_native_smoke smoke/smoke.cpp)
  target_link_libraries(metaxtract_native_smoke PRIVATE metaxtract_extract)
  add_test(NAME metaxtract_native_smoke COMMAND metaxtract_native_smoke)
endif()
