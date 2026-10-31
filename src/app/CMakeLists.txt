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
  ui/core/widget.cpp
  ui/core/layout.cpp
  ui/controls/controls.cpp
  ui/core/scroll.cpp
  ui/controls/edit.cpp
  ui/collections/list_view.cpp
  ui/collections/tree_view.cpp
  ui/popups/menu.cpp
  ui/popups/popup_layer.cpp
  ui/popups/menu_bar.cpp
  ui/controls/inputs.cpp
  ui/controls/tabs.cpp
  ui/controls/pickers.cpp
  ui/collections/tile_view.cpp
)
target_include_directories(bbapp PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(bbapp PUBLIC
  d2d1 dwrite d3d11 dxgi dcomp windowscodecs dwmapi
  shell32 ole32 oleaut32 uuid propsys shlwapi comctl32 imm32 uxtheme user32 gdi32
)

# bbviews: the main window's view over the FFI. Separate from bbapp so the widget tests stay free of the
# Rust core; the product and the harness link it.
add_library(bbviews STATIC
  views/bridge/core.cpp
  views/chrome/chrome.cpp
  views/chrome/command_ui.cpp
  views/dialogs/forms.cpp
  views/settings/settings_view.cpp
  views/chrome/palette_view.cpp
  views/main/sidebar.cpp
  views/dialogs/dialogs.cpp
  views/shell/thumbs.cpp
  views/shell/shell_icons.cpp
  views/shell/shell_props.cpp
  views/main/folder_tree.cpp
  views/panes/pane_images.cpp
  views/panes/code_view.cpp
  views/panes/image_view.cpp
  views/panes/font_view.cpp
  views/panes/media_player.cpp
  views/panes/media_pane.cpp
  views/panes/preview_pane.cpp
  views/panes/metadata_pane.cpp
  views/panes/property_host.cpp
  views/tools/tool_widgets.cpp
  views/tools/tool_state.cpp
  views/tools/tool_dialogs.cpp
  views/tools/tool_window.cpp
  views/dialogs/file_pickers.cpp
  views/tools/tool_banner.cpp
  views/main/main_view.cpp
  views/dialogs/ops_dialogs.cpp
  views/dialogs/ops_forms.cpp
  views/shell/file_drag.cpp
  views/dialogs/ops_menus.cpp
  views/shell/shell_menu_real.cpp
  views/settings/settings_ctxmenu.cpp
)
target_include_directories(bbviews PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}")
# Header and system libraries only: the Rust archive itself is linked by the executable, because the
# product and the harness link different builds of it (the harness's carries the test fixtures).
target_link_libraries(bbviews PUBLIC bbapp bbcore_abi mfplat mf mfuuid)

# The product. Never launched to check UI work; the harness is the way to see the interface.
add_executable(BertBrowser WIN32 app/main.cpp app/version.cpp app/app.rc)
target_link_libraries(BertBrowser PRIVATE bbviews bbcore)

# The elevated helpers sit beside the app and are found there and nowhere else: what they are
# started to do is the one thing that gets an administrator token.
add_custom_command(TARGET BertBrowser POST_BUILD
  COMMAND ${CMAKE_COMMAND} -E copy_if_different "${BB_INDEXER_EXE}" "${BB_ELEVATOR_EXE}" "$<TARGET_FILE_DIR:BertBrowser>"
  COMMENT "bb-indexer.exe and bb-elevator.exe beside BertBrowser.exe")

# Tests of the widget framework: plain C++ exe, no framework; renders offscreen on WARP.
enable_testing()
add_subdirectory(tests)
