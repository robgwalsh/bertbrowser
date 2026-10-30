cmake_minimum_required(VERSION 3.25)
project(bertbrowser VERSION 2.0.0 LANGUAGES CXX RC)

# Single-config generator (Ninja) through CMakePresets.json; build.ps1 enters the VS developer shell.
if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Debug)
endif()

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Static CRT. The Rust side is built with +crt-static (.cargo/config.toml); the two must match.
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
# cargo gets the native spelling: a forward-slash target dir becomes \\?\C:/... inside build
# scripts (embed-manifest), a path the linker cannot open, which failed both helpers with LNK1104.
file(TO_NATIVE_PATH "${CARGO_TARGET_DIR}" CARGO_TARGET_DIR_NATIVE)
set(BBCORE_LIB "${CARGO_TARGET_DIR}/${CARGO_PROFILE_DIR}/bbcore.lib")
set(BBCORE_HARNESS_LIB "${CARGO_TARGET_DIR}/${CARGO_PROFILE_DIR}/bbcore_harness.lib")
set(BBCORE_HEADER "${CMAKE_SOURCE_DIR}/include/bbcore.h")
set(BB_INDEXER_EXE "${CARGO_TARGET_DIR}/${CARGO_PROFILE_DIR}/bb-indexer.exe")
set(BB_ELEVATOR_EXE "${CARGO_TARGET_DIR}/${CARGO_PROFILE_DIR}/bb-elevator.exe")

# The product's staticlib: bb-ffi without its test fixtures, plus the two elevated helpers that sit
# beside BertBrowser.exe (found there and nowhere else: they get an administrator token).
add_custom_target(cargo_build ALL
  COMMAND ${CMAKE_COMMAND} -E env "CARGO_TARGET_DIR=${CARGO_TARGET_DIR_NATIVE}" cargo build -p bb-ffi -p bb-indexer -p bb-elevator ${CARGO_PROFILE_FLAG}
  COMMAND cbindgen --config cbindgen.toml --crate bb-ffi --output "${BBCORE_HEADER}"
  WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
  BYPRODUCTS "${BBCORE_LIB}" "${BBCORE_HEADER}" "${BB_INDEXER_EXE}" "${BB_ELEVATOR_EXE}"
  USES_TERMINAL
  COMMENT "cargo build -p bb-ffi -p bb-indexer -p bb-elevator (${CARGO_PROFILE_DIR}) and cbindgen")

# The harness's staticlib: the same ABI with the fixtures turned on (crate bb-ffi-harness). Two
# archives each carry their own copy of the Rust runtime, so an executable links exactly one of them.
# Built after the first so the two cargo runs never fight over the target directory lock.
add_custom_target(cargo_build_harness ALL
  COMMAND ${CMAKE_COMMAND} -E env "CARGO_TARGET_DIR=${CARGO_TARGET_DIR_NATIVE}" cargo build -p bb-ffi-harness ${CARGO_PROFILE_FLAG}
  WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
  BYPRODUCTS "${BBCORE_HARNESS_LIB}"
  USES_TERMINAL
  COMMENT "cargo build -p bb-ffi-harness (${CARGO_PROFILE_DIR})")
add_dependencies(cargo_build_harness cargo_build)

# The header and the system libraries Rust's crates link against (cargo reports them as
# native-static-libs); the include directory is all a library that only calls the ABI needs.
add_library(bbcore_abi INTERFACE)
target_include_directories(bbcore_abi INTERFACE "${CMAKE_SOURCE_DIR}/include")
target_link_libraries(bbcore_abi INTERFACE kernel32 advapi32 ntdll userenv ws2_32 bcrypt synchronization shell32 ole32 oleaut32 user32 secur32 crypt32 version)

add_dependencies(bbcore_abi cargo_build)

add_library(bbcore STATIC IMPORTED GLOBAL)
set_target_properties(bbcore PROPERTIES IMPORTED_LOCATION "${BBCORE_LIB}")
target_link_libraries(bbcore INTERFACE bbcore_abi)
add_dependencies(bbcore cargo_build)

add_library(bbcore_harness STATIC IMPORTED GLOBAL)
set_target_properties(bbcore_harness PROPERTIES IMPORTED_LOCATION "${BBCORE_HARNESS_LIB}")
target_link_libraries(bbcore_harness INTERFACE bbcore_abi)
add_dependencies(bbcore_harness cargo_build_harness)

add_subdirectory(src/app app)
add_subdirectory(harness)
