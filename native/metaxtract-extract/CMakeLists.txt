option(METAXTRACT_WARNINGS_AS_ERRORS "Treat native compiler warnings as errors" OFF)
set(METAXTRACT_SANITIZERS "" CACHE STRING "Native sanitizers: address,undefined")
option(METAXTRACT_CONFORMANCE "Enable the conformance-only fixture parser" OFF)
include(dependencies.cmake)

add_library(metaxtract_extract STATIC
  src/budget.cpp
  src/core.cpp
  src/detection.cpp
  src/images.cpp
  src/image_metadata.cpp
  src/heif.cpp
  src/raw.cpp
  src/audio.cpp
  src/media.cpp
  src/media_details.cpp
  src/tiff.cpp
  src/parser.cpp
  src/registry.cpp
)

target_include_directories(metaxtract_extract
  PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/include
)
target_compile_features(metaxtract_extract PUBLIC cxx_std_20)
target_link_libraries(metaxtract_extract PUBLIC mx_pugixml mx_zlib mx_libraw tag)
set_target_properties(metaxtract_extract PROPERTIES POSITION_INDEPENDENT_CODE ON)
install(TARGETS metaxtract_extract ARCHIVE DESTINATION lib)

if(METAXTRACT_CONFORMANCE)
  target_sources(metaxtract_extract PRIVATE ${PROJECT_SOURCE_DIR}/conformance/parser.cpp)
  target_compile_definitions(metaxtract_extract PRIVATE METAXTRACT_CONFORMANCE)
endif()

if(METAXTRACT_SANITIZERS)
  if(MSVC OR NOT METAXTRACT_SANITIZERS STREQUAL "address,undefined")
    message(FATAL_ERROR "Sanitizers require a GCC/Clang build with address,undefined")
  endif()
  target_compile_options(metaxtract_extract PUBLIC
    -fsanitize=address,undefined -fno-omit-frame-pointer)
  target_link_options(metaxtract_extract PUBLIC -fsanitize=address,undefined)
endif()

if(MSVC)
  target_compile_options(metaxtract_extract PUBLIC /EHsc)
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
  add_executable(metaxtract_native_smoke ${PROJECT_SOURCE_DIR}/conformance/native.cpp)
  target_link_libraries(metaxtract_native_smoke PRIVATE metaxtract_extract)
  if(WIN32)
    add_custom_command(TARGET metaxtract_native_smoke POST_BUILD
      COMMAND ${CMAKE_COMMAND} -E copy_if_different "$<TARGET_FILE:tag>" "$<TARGET_FILE_DIR:metaxtract_native_smoke>")
  endif()
  add_test(NAME metaxtract_native_smoke COMMAND metaxtract_native_smoke)
endif()
