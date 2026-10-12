option(METAXTRACT_WARNINGS_AS_ERRORS "Treat native compiler warnings as errors" OFF)
set(METAXTRACT_SANITIZERS "" CACHE STRING "Native sanitizers: address,undefined")
option(METAXTRACT_CONFORMANCE "Enable the conformance-only fixture parser" OFF)
include(dependencies.cmake)

# Compile PDF adapter objects without waiting for codec libraries to build.
add_library(mx_pdf_objects OBJECT src/pdf.cpp src/pdf_structure.cpp src/pdf_budget.cpp src/pdf_security.cpp)
target_include_directories(mx_pdf_objects PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/include")
target_include_directories(mx_pdf_objects SYSTEM PRIVATE "${qpdf_SOURCE_DIR}/include")
target_compile_features(mx_pdf_objects PRIVATE cxx_std_20)
set_target_properties(mx_pdf_objects PROPERTIES POSITION_INDEPENDENT_CODE ON)
if(METAXTRACT_SANITIZERS)
  target_compile_options(mx_pdf_objects PRIVATE -fsanitize=address,undefined -fno-omit-frame-pointer)
endif()
if(MSVC)
  target_compile_options(mx_pdf_objects PRIVATE /EHsc /W4 /permissive-)
  if(METAXTRACT_WARNINGS_AS_ERRORS)
    target_compile_options(mx_pdf_objects PRIVATE /WX)
  endif()
else()
  target_compile_options(mx_pdf_objects PRIVATE -Wall -Wextra -Wpedantic)
  if(METAXTRACT_WARNINGS_AS_ERRORS)
    target_compile_options(mx_pdf_objects PRIVATE -Werror)
  endif()
endif()

add_library(metaxtract_extract STATIC
  src/archive.cpp
  src/archive_budget.cpp
  src/budget.cpp
  src/binaries.cpp
  src/core.cpp
  src/detection.cpp
  src/images.cpp
  src/image_metadata.cpp
  src/xml.cpp
  src/zip.cpp
  src/documents.cpp
  src/text.cpp
  src/ole.cpp
  src/ole_properties.cpp
  src/ole_legacy.cpp
  src/mime.cpp
  src/email.cpp
  src/heif.cpp
  src/raw.cpp
  src/audio.cpp
  src/media.cpp
  src/media_details.cpp
  src/ffmpeg.cpp
  $<TARGET_OBJECTS:mx_pdf_objects>
  src/tiff.cpp
  src/parser.cpp
  src/registry.cpp
)

target_include_directories(metaxtract_extract
  PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/include
)
target_compile_features(metaxtract_extract PUBLIC cxx_std_20)
target_link_libraries(metaxtract_extract PUBLIC archive_static mx_pugixml mx_zlib mx_libraw tag mx_avformat mx_avcodec mx_avutil mx_swscale mx_swresample mx_qpdf)
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
    foreach(library IN ITEMS avformat avcodec avutil swscale swresample)
      add_custom_command(TARGET metaxtract_native_smoke POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E copy_if_different "$<TARGET_FILE:mx_${library}>" "$<TARGET_FILE_DIR:metaxtract_native_smoke>")
    endforeach()
  endif()
  add_test(NAME metaxtract_native_smoke COMMAND metaxtract_native_smoke)
endif()
