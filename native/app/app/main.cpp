#include <windows.h>

#include "app/version.h"

// Placeholder until M5. Returns at once so an accidental launch shows nothing.
int WINAPI wWinMain(HINSTANCE, HINSTANCE, PWSTR, int) {
    return bb::core_version().empty() ? 1 : 0;
}
