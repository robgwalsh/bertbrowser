#include <cstdio>

#include "app/version.h"

// Placeholder until M5. Keeps the exit-code contract: 64 is usage.
int main(int argc, char**) {
    std::printf("# bb-harness, core %ls\n", bb::core_version().c_str());
    if (argc < 2) {
        std::printf("usage: bb-harness --script <file.bbs> | -c <commands>\n");
        return 64;
    }
    std::printf("FAIL harness not implemented yet\n");
    return 1;
}
