metaxtract_source(FFmpeg)
find_package(Python3 REQUIRED COMPONENTS Interpreter)
file(SHA256 "${CMAKE_CURRENT_SOURCE_DIR}/build_ffmpeg.py" ffmpeg_builder_hash)
string(SHA256 ffmpeg_build_key "${ffmpeg_builder_hash};${FFmpeg_REVISION};${zlib_REVISION};${CMAKE_C_COMPILER};${CMAKE_C_COMPILER_VERSION};${CMAKE_SYSTEM_NAME};${CMAKE_SYSTEM_PROCESSOR};${METAXTRACT_STATIC_CRT};${METAXTRACT_SANITIZERS}")
set(ffmpeg_build "${PROJECT_SOURCE_DIR}/target/native-ffmpeg/${ffmpeg_build_key}")
set(ffmpeg_prefix "${ffmpeg_build}/install")
file(MAKE_DIRECTORY "${ffmpeg_prefix}/include")
add_custom_command(OUTPUT "${ffmpeg_build}/complete.json"
  COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/build_ffmpeg.py"
    --source "${FFmpeg_SOURCE_DIR}" --build "${ffmpeg_build}"
    --zlib "$<TARGET_FILE:mx_zlib>" --zlib-include "${zlib_SOURCE_DIR}"
    --cc "${CMAKE_C_COMPILER}" --sanitize "${METAXTRACT_SANITIZERS}"
    --static-crt "${METAXTRACT_STATIC_CRT}"
  DEPENDS mx_zlib "${CMAKE_CURRENT_SOURCE_DIR}/build_ffmpeg.py"
  VERBATIM)
add_custom_target(mx_ffmpeg_build DEPENDS "${ffmpeg_build}/complete.json")
foreach(library IN ITEMS avformat avcodec avutil swscale swresample)
  if(library STREQUAL "avutil")
    file(READ "${FFmpeg_SOURCE_DIR}/libavutil/version.h" version_header)
  else()
    file(READ "${FFmpeg_SOURCE_DIR}/lib${library}/version_major.h" version_header)
  endif()
  string(TOUPPER "${library}" library_upper)
  string(REGEX MATCH "#define LIB${library_upper}_VERSION_MAJOR +([0-9]+)" version_match "${version_header}")
  set(major "${CMAKE_MATCH_1}")
  if(NOT major)
    message(FATAL_ERROR "FFmpeg library major version missing: ${library}")
  endif()
  add_library(mx_${library} SHARED IMPORTED GLOBAL)
  if(WIN32)
    set_target_properties(mx_${library} PROPERTIES
      IMPORTED_LOCATION "${ffmpeg_prefix}/lib/${library}-${major}.dll"
      IMPORTED_IMPLIB "${ffmpeg_prefix}/lib/${library}.lib")
  elseif(APPLE)
    set_target_properties(mx_${library} PROPERTIES IMPORTED_LOCATION "${ffmpeg_prefix}/lib/lib${library}.${major}.dylib")
  else()
    set_target_properties(mx_${library} PROPERTIES IMPORTED_LOCATION "${ffmpeg_prefix}/lib/lib${library}.so.${major}")
  endif()
  set_target_properties(mx_${library} PROPERTIES INTERFACE_INCLUDE_DIRECTORIES "${ffmpeg_prefix}/include")
  add_dependencies(mx_${library} mx_ffmpeg_build)
endforeach()
install(DIRECTORY "${ffmpeg_prefix}/lib/" DESTINATION lib
  FILES_MATCHING PATTERN "*.dll" PATTERN "*.lib" PATTERN "*.so*" PATTERN "*.dylib")
install(FILES "${FFmpeg_SOURCE_DIR}/COPYING.LGPLv2.1" DESTINATION share/metaxtract/licenses RENAME FFmpeg-LGPL.txt)
install(FILES "${ffmpeg_build}/reviewed-configuration.json" DESTINATION share/metaxtract/licenses RENAME FFmpeg-configuration.json)
