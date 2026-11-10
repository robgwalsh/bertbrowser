# Runs cargo for the cmake build with the environment a fresh shell would give it.
#   cmake -DCARGO_TARGET_DIR=<dir> -P tools/cargo.cmake -- build -p bb-ffi ...
#
# build.ps1 enters the VS developer shell, which sets INCLUDE, LIB and friends. The C build scripts
# under our dependencies (sqlite, mimalloc, ring, unrar, lzma) rerun whenever one of those differs
# from their last run, and then everything above them recompiles - bb-core included. So a plain
# `cargo test` after a build, or a build after a plain `cargo test`, cost a whole bb-core rebuild.
# Here each such variable is put back to what a new shell has: the user's value, else the machine's,
# else none. `cc` finds MSVC on its own without them, as it does for a plain cargo command.
# A script rather than `cmake -E env` arguments: these values are path lists that end in a
# backslash, which CMake's list escaping mangles.
foreach(var INCLUDE LIB LIBPATH EXTERNAL_INCLUDE VCINSTALLDIR VCToolsVersion WindowsSdkDir
            WindowsSDKVersion VSCMD_ARG_VCVARS_SPECTRE)
  cmake_host_system_information(RESULT value QUERY WINDOWS_REGISTRY "HKCU/Environment" VALUE ${var})
  if("${value}" STREQUAL "")
    cmake_host_system_information(RESULT value QUERY WINDOWS_REGISTRY
      "HKLM/SYSTEM/CurrentControlSet/Control/Session Manager/Environment" VALUE ${var})
  endif()
  if("${value}" STREQUAL "")
    unset(ENV{${var}})
  else()
    set(ENV{${var}} "${value}")
  endif()
endforeach()

if(DEFINED CARGO_TARGET_DIR)
  set(ENV{CARGO_TARGET_DIR} "${CARGO_TARGET_DIR}")
endif()

# Everything after `--` is cargo's.
set(args "")
set(passing FALSE)
math(EXPR last "${CMAKE_ARGC} - 1")
foreach(i RANGE ${last})
  if(passing)
    list(APPEND args "${CMAKE_ARGV${i}}")
  elseif("${CMAKE_ARGV${i}}" STREQUAL "--")
    set(passing TRUE)
  endif()
endforeach()

execute_process(COMMAND cargo ${args} RESULT_VARIABLE result)
if(NOT result EQUAL 0)
  message(FATAL_ERROR "cargo ${args} failed: ${result}")
endif()
