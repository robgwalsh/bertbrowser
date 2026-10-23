# bb-harness: the offscreen UI harness. Links bbapp with the harness fakes; runs tools/ui/*.bbs.
add_executable(bb-harness main.cpp)
target_link_libraries(bb-harness PRIVATE bbapp)
