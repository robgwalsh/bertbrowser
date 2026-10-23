# bbapp: everything but WinMain, so the harness links the same code as the product.
add_library(bbapp STATIC
  app/version.cpp
)
target_include_directories(bbapp PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(bbapp PUBLIC
  bbcore
  d2d1 dwrite d3d11 dxgi windowscodecs dwmapi
  shell32 ole32 oleaut32 uuid propsys shlwapi comctl32 imm32 uxtheme user32 gdi32
)

# The product. Never launched to check UI work; the harness is the way to see the interface.
add_executable(BertBrowser WIN32 app/main.cpp)
target_link_libraries(BertBrowser PRIVATE bbapp)
