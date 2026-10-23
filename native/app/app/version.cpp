#include "app/version.h"

#include "bbcore.h"

namespace bb {
std::wstring core_version() {
    const BbStr s = bb_version();
    return std::wstring(reinterpret_cast<const wchar_t*>(s.ptr), s.len);
}
}  // namespace bb
