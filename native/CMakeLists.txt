cmake_minimum_required(VERSION 3.25)
project(bertbrowser VERSION 2.0.0 LANGUAGES CXX)

# Single-config generator (Ninja) through CMakePresets.json; build.ps1 enters the VS developer shell.
if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Debug)
endif()

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Static CRT. The Rust side is built with +crt-static (native/.cargo/config.toml); the two must match.
# The release static CRT in every configuration: the C and C++ sources inside Rust dependencies (unrar,
# sevenz) are compiled by cargo's `cc` with /MT even in a debug profile, and a debug-CRT executable
# (/MTd, _ITERATOR_DEBUG_LEVEL=2) cannot link them (LNK2038).
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded")

add_compile_options(/W4 /WX /permissive- /utf-8 /EHsc /Zc:__cplusplus /Zc:preprocessor)
add_compile_definitions(UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX STRICT)
add_link_options(/WX)

# ---- Rust: cargo builds the staticlib, cbindgen writes the header -------------------------------
if(CMAKE_BUILD_TYPE STREQUAL "Debug")
  set(CARGO_PROFILE_FLAG "")
  set(CARGO_PROFILE_DIR "debug")
else()
  set(CARGO_PROFILE_FLAG "--release")
  set(CARGO_PROFILE_DIR "release")
endif()
set(CARGO_TARGET_DIR "${CMAKE_SOURCE_DIR}/target")
set(BBCORE_LIB "${CARGO_TARGET_DIR}/${CARGO_PROFILE_DIR}/bbcore.lib")
set(BBCORE_HEADER "${CMAKE_SOURCE_DIR}/include/bbcore.h")

add_custom_target(cargo_build ALL
  COMMAND ${CMAKE_COMMAND} -E env "CARGO_TARGET_DIR=${CARGO_TARGET_DIR}" cargo build -p bb-ffi ${CARGO_PROFILE_FLAG}
  COMMAND cbindgen --config cbindgen.toml --crate bb-ffi --output "${BBCORE_HEADER}"
  WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
  BYPRODUCTS "${BBCORE_LIB}" "${BBCORE_HEADER}"
  USES_TERMINAL
  COMMENT "cargo build -p bb-ffi (${CARGO_PROFILE_DIR}) and cbindgen")

add_library(bbcore STATIC IMPORTED GLOBAL)
set_target_properties(bbcore PROPERTIES IMPORTED_LOCATION "${BBCORE_LIB}")
target_include_directories(bbcore INTERFACE "${CMAKE_SOURCE_DIR}/include")
# What the Rust standard library and its crates link against (cargo reports them as native-static-libs).
target_link_libraries(bbcore INTERFACE kernel32 advapi32 ntdll userenv ws2_32 bcrypt synchronization)
add_dependencies(bbcore cargo_build)

add_subdirectory(app)
add_subdirectory(harness)
add_subdirectory(spikes)
