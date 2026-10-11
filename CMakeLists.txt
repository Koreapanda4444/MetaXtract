cmake_minimum_required(VERSION 3.24)

project(MetaXtractNative LANGUAGES C CXX)

# Rust uses the release MSVC runtime for all Cargo profiles. Keep standalone
# native builds on the same runtime so linked static libraries agree.
if(MSVC AND NOT DEFINED CMAKE_MSVC_RUNTIME_LIBRARY)
  if(METAXTRACT_STATIC_CRT)
    set(CMAKE_MSVC_RUNTIME_LIBRARY MultiThreaded)
  else()
    set(CMAKE_MSVC_RUNTIME_LIBRARY MultiThreadedDLL)
  endif()
endif()

include(CTest)
add_subdirectory(native/metaxtract-extract)
