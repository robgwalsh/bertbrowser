# bbapp_tests: no external framework. Builds widget trees on a BitmapSurface over WARP with a parked
# window; nothing is ever shown. Writes PNGs next to the exe (details_grid_10k.png, widget_gallery.png).
add_executable(bbapp_tests
  main.cpp
  gfx_tests.cpp
  platform_tests.cpp
  ui_tests.cpp
  edit_tests.cpp
  list_tests.cpp
)
target_link_libraries(bbapp_tests PRIVATE bbapp)
add_test(NAME bbapp_tests COMMAND bbapp_tests)

# Run after every build, so a build that compiles but breaks the framework fails here.
add_custom_command(TARGET bbapp_tests POST_BUILD
  COMMAND bbapp_tests
  COMMENT "running bbapp_tests"
  VERBATIM)
