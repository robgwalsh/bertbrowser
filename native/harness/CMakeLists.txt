# bb-harness: the offscreen UI harness. Links bbapp with the harness fakes; runs tools/ui/*.bbs.
# bbapp itself has no Rust dependency yet; the harness takes bbcore directly until the views milestone.
add_executable(bb-harness main.cpp ../app/app/version.cpp)
target_link_libraries(bb-harness PRIVATE bbapp bbcore)
