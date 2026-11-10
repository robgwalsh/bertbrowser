cmake_minimum_required(VERSION 3.25)

# Debug links with LLD's lld-link when it is installed (Visual Studio's "C++ Clang tools for Windows"
# component puts it under VC\Tools\Llvm), else MSVC's link.exe. A debug link pulls in the whole Rust
# archive with its debug info, and lld-link writes that PDB several times faster. Release keeps
# link.exe: it is what ships. -DBB_LLD_LINK=<path to lld-link.exe> picks one; -DBB_LLD_LINK=OFF
# turns it off. Decided at the first configure (it is the linker CMake records), so changing it
# wants a fresh build directory.
if(CMAKE_BUILD_TYPE STREQUAL "Debug" OR NOT CMAKE_BUILD_TYPE)
  find_program(BB_LLD_LINK lld-link HINTS "$ENV{VCINSTALLDIR}Tools/Llvm/x64/bin"
    DOC "lld-link.exe for Debug links; OFF for MSVC's link.exe")
  if(BB_LLD_LINK AND NOT CMAKE_LINKER)
    set(CMAKE_LINKER "${BB_LLD_LINK}" CACHE FILEPATH "Linker")
  endif()
endif()

project(bertbrowser VERSION 2.0.0 LANGUAGES CXX RC)
message(STATUS "Linker: ${CMAKE_LINKER}")

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

# Debug info inside each object (/Z7) rather than one shared PDB per target (/Zi): with /Zi every
# parallel compile writes that PDB through one mspdbsrv process, which serialises them.
set(CMAKE_MSVC_DEBUG_INFORMATION_FORMAT "$<$<CONFIG:Debug,RelWithDebInfo>:Embedded>")

# The precompiled header every C++ target uses (src/app/pch.h says what belongs in it).
set(BB_PCH "${CMAKE_SOURCE_DIR}/src/app/pch.h")
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
# cargo runs through tools/cargo.cmake, which puts back the environment a fresh shell has: the VS
# developer shell's INCLUDE/LIB would otherwise make the C build scripts under our dependencies rerun,
# and bb-core recompile, every time the build and a plain cargo command take turns.
set(BB_CARGO "${CMAKE_COMMAND}" "-DCARGO_TARGET_DIR=${CARGO_TARGET_DIR_NATIVE}" -P "${CMAKE_SOURCE_DIR}/tools/cargo.cmake" --)

# The header first, on its own: cbindgen reads bb-ffi's source and compiles nothing, so it takes a
# second or two, and the C++ compiles need only it. They run while cargo builds; only the links wait
# for the Rust archives (below).
add_custom_target(bbcore_header ALL
  COMMAND cbindgen --config cbindgen.toml --crate bb-ffi --output "${BBCORE_HEADER}"
  WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
  BYPRODUCTS "${BBCORE_HEADER}"
  VERBATIM
  COMMENT "cbindgen -> include/bbcore.h")

# The product's staticlib: bb-ffi without its test fixtures, plus the two elevated helpers that sit
# beside BertBrowser.exe (found there and nowhere else: they get an administrator token). After the
# header, so cbindgen's `cargo metadata` never waits on this build's locks.
add_custom_target(cargo_build ALL
  COMMAND ${BB_CARGO} build -p bb-ffi -p bb-indexer -p bb-elevator ${CARGO_PROFILE_FLAG}
  WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
  BYPRODUCTS "${BBCORE_LIB}" "${BB_INDEXER_EXE}" "${BB_ELEVATOR_EXE}"
  USES_TERMINAL VERBATIM
  COMMENT "cargo build -p bb-ffi -p bb-indexer -p bb-elevator (${CARGO_PROFILE_DIR})")
add_dependencies(cargo_build bbcore_header)

# The harness's staticlib: the same ABI with the fixtures turned on (crate bb-ffi-harness). Two
# archives each carry their own copy of the Rust runtime, so an executable links exactly one of them.
# Built after the first so the two cargo runs never fight over the target directory lock.
add_custom_target(cargo_build_harness ALL
  COMMAND ${BB_CARGO} build -p bb-ffi-harness ${CARGO_PROFILE_FLAG}
  WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
  BYPRODUCTS "${BBCORE_HARNESS_LIB}"
  USES_TERMINAL VERBATIM
  COMMENT "cargo build -p bb-ffi-harness (${CARGO_PROFILE_DIR})")
add_dependencies(cargo_build_harness cargo_build)

# The header and the system libraries Rust's crates link against (cargo reports them as
# native-static-libs); the include directory is all a library that only calls the ABI needs.
add_library(bbcore_abi INTERFACE)
target_include_directories(bbcore_abi INTERFACE "${CMAKE_SOURCE_DIR}/include")
target_link_libraries(bbcore_abi INTERFACE kernel32 advapi32 ntdll userenv ws2_32 bcrypt synchronization shell32 ole32 oleaut32 user32 secur32 crypt32 version)

add_dependencies(bbcore_abi bbcore_header)

# No target dependency on the cargo builds here: one would make every object of every consumer wait
# for cargo too (Ninja orders a target's compiles after its dependencies). The archive is the link
# step's input and a declared byproduct of its cargo target, so Ninja runs the link after it anyway.
add_library(bbcore STATIC IMPORTED GLOBAL)
set_target_properties(bbcore PROPERTIES IMPORTED_LOCATION "${BBCORE_LIB}")
target_link_libraries(bbcore INTERFACE bbcore_abi)

add_library(bbcore_harness STATIC IMPORTED GLOBAL)
set_target_properties(bbcore_harness PROPERTIES IMPORTED_LOCATION "${BBCORE_HARNESS_LIB}")
target_link_libraries(bbcore_harness INTERFACE bbcore_abi)

add_subdirectory(src/app app)
add_subdirectory(harness)
