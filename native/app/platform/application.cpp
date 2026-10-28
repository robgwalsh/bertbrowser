#include "platform/application.h"

#include <ole2.h>

namespace bb::platform {

bool init_process() {
    static bool done = false;
    if (done) {
        return true;
    }
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);  // refused if a manifest already chose
    const HRESULT hr = OleInitialize(nullptr);
    done = SUCCEEDED(hr) || hr == RPC_E_CHANGED_MODE;
    return done;
}

int run_message_loop() {
    MSG msg;
    while (GetMessageW(&msg, nullptr, 0, 0) > 0) {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    return static_cast<int>(msg.wParam);
}

int pump_messages() {
    int n = 0;
    MSG msg;
    while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) {
        if (msg.message == WM_QUIT) {
            break;
        }
        DispatchMessageW(&msg);
        ++n;
    }
    return n;
}

void post_quit(int code) { PostQuitMessage(code); }

}  // namespace bb::platform
