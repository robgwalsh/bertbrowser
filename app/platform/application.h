// Process-level setup and the message loop.
#pragma once
#include <windows.h>

namespace bb::platform {

// Per-monitor v2 DPI awareness, OLE (clipboard, drag and drop, WIC). Safe to call twice.
bool init_process();
// GetMessage / TranslateMessage / DispatchMessage until WM_QUIT; returns its exit code.
int run_message_loop();
// Dispatch everything queued without blocking and without translating: WM_CHAR is posted explicitly by
// whoever drives the window, as the harness does. Returns the number of messages handled.
int pump_messages();
void post_quit(int code = 0);

}  // namespace bb::platform
