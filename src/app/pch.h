// The precompiled header of every C++ target (target_precompile_headers in the CMakeLists): the
// system and standard headers nearly every file pulls in, directly or through gfx/ and ui/. Parsing
// <windows.h>, the shell and Direct2D headers once per target rather than once per file is most of
// a compile here. Only headers that never change belong in it - never one of this project's own, or
// every edit to that header recompiles everything. Files still include what they use: this is an
// optimisation, not where anything is declared.
#pragma once

#include <windows.h>

#include <d2d1_1.h>
#include <d3d11.h>
#include <dwmapi.h>
#include <dwrite_3.h>
#include <shellapi.h>
#include <shlobj.h>
#include <shobjidl.h>
#include <wrl/client.h>

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cwctype>
#include <filesystem>
#include <fstream>
#include <functional>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <thread>
#include <unordered_map>
#include <utility>
#include <vector>
