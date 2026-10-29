# bb-harness: the offscreen UI harness. A separate exe over the same views as the product, with the
# core built on its harness fakes; runs tools/ui/*.bbs. Never a mode of BertBrowser.exe.
add_executable(bb-harness
  main.cpp
  options.cpp
  sandbox.cpp
  session.cpp
  runner.cpp
  bench.cpp
  runner_bench.cpp
  runner_ui.cpp
  runner_panes.cpp
  runner_tools.cpp
  runner_ops.cpp
  ../app/app/version.cpp
)
target_include_directories(bb-harness PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(bb-harness PRIVATE bbviews psapi winmm)
