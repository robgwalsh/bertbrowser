# Phase 0 spikes: one small exe per risk. Deleted after M5.
add_executable(s7_ffi s7_ffi/main.cpp)
target_link_libraries(s7_ffi PRIVATE bbcore)

add_executable(s4_capture s4_capture/main.cpp)
target_link_libraries(s4_capture PRIVATE d2d1 dwrite d3d11 dxgi windowscodecs ole32 user32)
