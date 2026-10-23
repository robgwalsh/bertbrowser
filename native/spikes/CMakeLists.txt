# Phase 0 spikes: one small exe per risk. Deleted after M5.
add_executable(s7_ffi s7_ffi/main.cpp)
target_link_libraries(s7_ffi PRIVATE bbcore)

add_executable(s4_capture s4_capture/main.cpp)
target_link_libraries(s4_capture PRIVATE d2d1 dwrite d3d11 dxgi windowscodecs ole32 user32)

add_executable(s1_edit s1_edit/main.cpp)
target_link_libraries(s1_edit PRIVATE d2d1 dwrite d3d11 dxgi windowscodecs ole32 user32 imm32)

add_executable(s3_shellmenu s3_shellmenu/main.cpp)
target_link_libraries(s3_shellmenu PRIVATE shell32 ole32 user32 gdi32 advapi32)

add_executable(s2_media s2_media/main.cpp)
target_link_libraries(s2_media PRIVATE mfplat mfuuid mf mfreadwrite d3d11 dxgi ole32 oleaut32 user32)

add_executable(s8_chrome s8_chrome/main.cpp)
target_link_libraries(s8_chrome PRIVATE dwmapi user32 gdi32)
