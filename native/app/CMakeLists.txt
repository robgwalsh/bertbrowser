# bbapp: everything but WinMain, so the harness links the same code as the product.
# The UI foundation (platform/, gfx/, ui/) has no dependency on the Rust core; the views milestone adds it.
add_library(bbapp STATIC
  platform/application.cpp
  platform/window.cpp
  platform/popup_window.cpp
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
  ui/menu.cpp
  ui/popup_layer.cpp
  ui/menu_bar.cpp
  ui/inputs.cpp
  ui/tabs.cpp
  ui/pickers.cpp
  ui/tile_view.cpp
)
target_include_directories(bbapp PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(bbapp PUBLIC
  d2d1 dwrite d3d11 dxgi windowscodecs dwmapi
  shell32 ole32 oleaut32 uuid propsys shlwapi comctl32 imm32 uxtheme user32 gdi32
)

# bbviews: the main window's view over the FFI. Separate from bbapp so the widget tests stay free of the
# Rust core; the product and the harness link it.
add_library(bbviews STATIC
  views/core.cpp
  views/chrome.cpp
  views/command_ui.cpp
  views/forms.cpp
  views/settings_view.cpp
  views/palette_view.cpp
  views/sidebar.cpp
  views/dialogs.cpp
  views/thumbs.cpp
  views/shell_props.cpp
  views/folder_tree.cpp
  views/pane_images.cpp
  views/code_view.cpp
  views/image_view.cpp
  views/font_view.cpp
  views/media_player.cpp
  views/media_pane.cpp
  views/preview_pane.cpp
  views/metadata_pane.cpp
  views/property_host.cpp
  views/tool_widgets.cpp
  views/tool_state.cpp
  views/tool_dialogs.cpp
  views/tool_banner.cpp
  views/main_view.cpp
  views/ops_dialogs.cpp
  views/ops_forms.cpp
  views/file_drag.cpp
  views/ops_menus.cpp
  views/shell_menu_real.cpp
  views/settings_ctxmenu.cpp
)
target_include_directories(bbviews PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}")
# Header and system libraries only: the Rust archive itself is linked by the executable, because the
# product and the harness link different builds of it (the harness's carries the test fixtures).
target_link_libraries(bbviews PUBLIC bbapp bbcore_abi mfplat mf mfuuid)

# The product. Never launched to check UI work; the harness is the way to see the interface.
add_executable(BertBrowser WIN32 app/main.cpp app/version.cpp)
target_link_libraries(BertBrowser PRIVATE bbviews bbcore)

# The elevated helpers sit beside the app and are found there and nowhere else: what they are
# started to do is the one thing that gets an administrator token.
add_custom_command(TARGET BertBrowser POST_BUILD
  COMMAND ${CMAKE_COMMAND} -E copy_if_different "${BB_INDEXER_EXE}" "${BB_ELEVATOR_EXE}" "$<TARGET_FILE_DIR:BertBrowser>"
  COMMENT "bb-indexer.exe and bb-elevator.exe beside BertBrowser.exe")

# Tests of the widget framework: plain C++ exe, no framework; renders offscreen on WARP.
enable_testing()
add_subdirectory(tests)
