# bbapp: everything but WinMain, so the harness links the same code as the product.
# The UI foundation (platform/, gfx/, ui/) has no dependency on the Rust core; the views milestone adds it.
add_library(bbapp STATIC
  platform/application.cpp
  platform/window.cpp
  gfx/device.cpp
  gfx/palette.cpp
  gfx/text_cache.cpp
  gfx/surface.cpp
  ui/widget.cpp
  ui/layout.cpp
  ui/controls.cpp
  ui/scroll.cpp
  ui/edit.cpp
  ui/list_view.cpp
  ui/tree_view.cpp
)
target_include_directories(bbapp PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(bbapp PUBLIC
  d2d1 dwrite d3d11 dxgi windowscodecs dwmapi
  shell32 ole32 oleaut32 uuid propsys shlwapi comctl32 imm32 uxtheme user32 gdi32
)

# The product. Never launched to check UI work; the harness is the way to see the interface.
add_executable(BertBrowser WIN32 app/main.cpp app/version.cpp)
target_link_libraries(BertBrowser PRIVATE bbapp bbcore)

# Tests of the widget framework: plain C++ exe, no framework; renders offscreen on WARP.
enable_testing()
add_subdirectory(tests)
