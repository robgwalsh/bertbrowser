// Spike S3: host a real shell context-menu extension the way the app does (App/Interop/
// ShellContextMenu.cs, ShellSelection.cs, ShellMenuIcons.cs): per extension, in a private HMENU that
// is never shown, WM_INITMENUPOPUP delivered by hand before each submenu is walked, icons read out
// with GetDIBits so their premultiplied alpha survives, verbs via GetCommandString. Nothing is
// invoked: what 7-Zip does when clicked is 7-Zip's business, and this spike must not start anything.
//
// It needs a handler to be installed. It looks for 7-Zip under HKCR\*\shellex\ContextMenuHandlers
// and falls back to the first handler registered there whose default value is a CLSID.
#include <windows.h>

#include <shlobj.h>
#include <shobjidl.h>
#include <wrl/client.h>

#include <cstdio>
#include <string>
#include <vector>

using Microsoft::WRL::ComPtr;

namespace {

int failures = 0;

void check(bool ok, const char* what) {
    if (!ok) {
        ++failures;
        std::printf("S3 FAIL: %s\n", what);
    }
}

struct Entry {
    std::wstring text;
    bool separator = false;
    bool enabled = true;
    bool submenu = false;
    UINT id = 0;
    int icon_w = 0, icon_h = 0;
    size_t icon_opaque = 0;
    std::wstring verb;
    std::vector<Entry> children;
};

// ---- registry: the handler to host -------------------------------------------------------------

std::wstring read_default(HKEY root, const std::wstring& sub) {
    HKEY key{};
    if (RegOpenKeyExW(root, sub.c_str(), 0, KEY_READ, &key) != ERROR_SUCCESS) {
        return {};
    }
    wchar_t buffer[128]{};
    DWORD bytes = sizeof(buffer);
    DWORD type = 0;
    const LSTATUS st = RegQueryValueExW(key, nullptr, nullptr, &type, reinterpret_cast<BYTE*>(buffer), &bytes);
    RegCloseKey(key);
    return st == ERROR_SUCCESS && type == REG_SZ ? std::wstring(buffer) : std::wstring();
}

// A handler key's default value is a CLSID, or the key's own name is one.
bool find_handler(std::wstring& name, CLSID& clsid) {
    const std::wstring base = L"*\\shellex\\ContextMenuHandlers";
    HKEY key{};
    if (RegOpenKeyExW(HKEY_CLASSES_ROOT, base.c_str(), 0, KEY_READ, &key) != ERROR_SUCCESS) {
        return false;
    }
    std::vector<std::wstring> names;
    for (DWORD i = 0;; ++i) {
        wchar_t sub[256]{};
        DWORD len = 256;
        if (RegEnumKeyExW(key, i, sub, &len, nullptr, nullptr, nullptr, nullptr) != ERROR_SUCCESS) break;
        names.emplace_back(sub);
    }
    RegCloseKey(key);
    auto try_name = [&](const std::wstring& n) {
        std::wstring value = read_default(HKEY_CLASSES_ROOT, base + L"\\" + n);
        if (value.empty()) value = n;
        if (SUCCEEDED(CLSIDFromString(value.c_str(), &clsid))) {
            name = n;
            return true;
        }
        return false;
    };
    for (const auto& n : names) {
        if (n == L"7-Zip" && try_name(n)) return true;
    }
    for (const auto& n : names) {
        if (try_name(n)) return true;
    }
    return false;
}

// ---- selection: folder PIDL + IDataObject, as ShellSelection.cs -------------------------------

struct Selection {
    PIDLIST_ABSOLUTE folder = nullptr;
    std::vector<PIDLIST_ABSOLUTE> items;
    ComPtr<IDataObject> data;

    ~Selection() {
        for (auto p : items) ILFree(p);
        if (folder) ILFree(folder);
    }

    bool create(const std::wstring& folder_path, const std::vector<std::wstring>& paths) {
        SFGAOF attrs = 0;
        if (FAILED(SHParseDisplayName(folder_path.c_str(), nullptr, &folder, 0, &attrs))) return false;
        for (const auto& p : paths) {
            PIDLIST_ABSOLUTE pidl = nullptr;
            if (SUCCEEDED(SHParseDisplayName(p.c_str(), nullptr, &pidl, 0, &attrs))) items.push_back(pidl);
        }
        if (!paths.empty() && items.empty()) return false;
        if (!items.empty()) {
            ComPtr<IShellItemArray> array;
            std::vector<PCIDLIST_ABSOLUTE> raw(items.begin(), items.end());
            if (FAILED(SHCreateShellItemArrayFromIDLists(static_cast<UINT>(raw.size()), raw.data(), &array))) {
                return false;
            }
            if (FAILED(array->BindToHandler(nullptr, BHID_DataObject, IID_PPV_ARGS(&data)))) return false;
        }
        return true;
    }
};

// ---- icons: GetDIBits, as ShellMenuIcons.cs ---------------------------------------------------

void read_icon(HBITMAP hbmp, Entry& e) {
    if (reinterpret_cast<ULONG_PTR>(hbmp) <= 11) return;  // HBMMENU_* sentinels
    BITMAP bm{};
    if (GetObjectW(hbmp, sizeof(bm), &bm) == 0) return;
    if (bm.bmWidth <= 0 || bm.bmHeight <= 0 || bm.bmWidth > 256 || bm.bmHeight > 256) return;
    BITMAPINFO info{};
    info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    info.bmiHeader.biWidth = bm.bmWidth;
    info.bmiHeader.biHeight = -bm.bmHeight;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    std::vector<BYTE> pixels(static_cast<size_t>(bm.bmWidth) * bm.bmHeight * 4);
    const HDC hdc = GetDC(nullptr);
    const int rows = GetDIBits(hdc, hbmp, 0, static_cast<UINT>(bm.bmHeight), pixels.data(), &info, DIB_RGB_COLORS);
    ReleaseDC(nullptr, hdc);
    if (rows == 0) return;
    e.icon_w = bm.bmWidth;
    e.icon_h = bm.bmHeight;
    for (size_t i = 3; i < pixels.size(); i += 4) {
        if (pixels[i] != 0) ++e.icon_opaque;
    }
    // A bitmap with no alpha channel reads back all-zero alpha; the app then treats it as opaque.
    if (bm.bmBitsPixel < 32 || e.icon_opaque == 0) e.icon_opaque = pixels.size() / 4;
}

// ---- the host ---------------------------------------------------------------------------------

constexpr UINT kIdFirst = 1, kIdLast = 0x7FFF;

struct Host {
    ComPtr<IContextMenu> menu;
    ComPtr<IContextMenu2> menu2;
    ComPtr<IContextMenu3> menu3;
    HMENU hmenu = nullptr;

    ~Host() {
        if (hmenu) DestroyMenu(hmenu);
    }

    bool create(const CLSID& clsid, Selection& sel) {
        ComPtr<IUnknown> unk;
        if (FAILED(CoCreateInstance(clsid, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&unk)))) return false;
        ComPtr<IShellExtInit> init;
        if (FAILED(unk.As(&init)) || FAILED(unk.As(&menu))) return false;
        if (FAILED(init->Initialize(sel.folder, sel.data.Get(), nullptr))) return false;
        unk.As(&menu2);
        unk.As(&menu3);
        return true;
    }

    std::vector<Entry> query() {
        hmenu = CreatePopupMenu();
        const HRESULT hr = menu->QueryContextMenu(hmenu, 0, kIdFirst, kIdLast, CMF_NORMAL);
        if (FAILED(hr)) return {};
        return walk(hmenu);
    }

    void handle(UINT msg, WPARAM wp, LPARAM lp) {
        if (menu3) {
            LRESULT r = 0;
            menu3->HandleMenuMsg2(msg, wp, lp, &r);
        } else if (menu2) {
            menu2->HandleMenuMsg(msg, wp, lp);
        }
    }

    std::wstring verb_of(UINT id) {
        wchar_t buffer[256]{};
        if (menu->GetCommandString(id - kIdFirst, GCS_VERBW, nullptr, reinterpret_cast<CHAR*>(buffer), 256) == S_OK) {
            return buffer;
        }
        return {};
    }

    std::vector<Entry> walk(HMENU m) {
        std::vector<Entry> out;
        const int count = GetMenuItemCount(m);
        for (int pos = 0; pos < count; ++pos) {
            MENUITEMINFOW info{};
            info.cbSize = sizeof(info);
            info.fMask = MIIM_STATE | MIIM_ID | MIIM_SUBMENU | MIIM_STRING | MIIM_BITMAP | MIIM_FTYPE;
            if (!GetMenuItemInfoW(m, static_cast<UINT>(pos), TRUE, &info)) continue;
            Entry e;
            if (info.fType & MFT_SEPARATOR) {
                e.separator = true;
                out.push_back(e);
                continue;
            }
            if (!(info.fType & (MFT_BITMAP | MFT_OWNERDRAW)) && info.cch > 0) {
                std::wstring text(info.cch + 1, L'\0');
                MENUITEMINFOW t{};
                t.cbSize = sizeof(t);
                t.fMask = MIIM_STRING;
                t.dwTypeData = text.data();
                t.cch = info.cch + 1;
                if (GetMenuItemInfoW(m, static_cast<UINT>(pos), TRUE, &t)) {
                    text.resize(t.cch);
                    e.text = text;
                }
            }
            e.enabled = (info.fState & (MFS_DISABLED | MFS_GRAYED)) == 0;
            e.id = info.wID;
            read_icon(info.hbmpItem, e);
            if (info.hSubMenu) {
                handle(WM_INITMENUPOPUP, reinterpret_cast<WPARAM>(info.hSubMenu), static_cast<LPARAM>(pos));
                e.submenu = true;
                e.children = walk(info.hSubMenu);
            } else {
                if ((info.fType & MFT_OWNERDRAW) && e.text.empty()) e.text = verb_of(info.wID);
                e.verb = verb_of(info.wID);
            }
            out.push_back(std::move(e));
        }
        return out;
    }
};

void print(const std::vector<Entry>& entries, int depth, size_t& items, size_t& icons, size_t& submenus) {
    for (const auto& e : entries) {
        std::printf("%*s", depth * 2, "");
        if (e.separator) {
            std::printf("----\n");
            continue;
        }
        ++items;
        if (e.icon_w) ++icons;
        if (e.submenu) ++submenus;
        std::printf("%ls%s%s", e.text.c_str(), e.enabled ? "" : " (disabled)", e.submenu ? " >" : "");
        if (!e.verb.empty()) std::printf("  [verb %ls]", e.verb.c_str());
        if (e.icon_w) std::printf("  [icon %dx%d, %zu opaque px]", e.icon_w, e.icon_h, e.icon_opaque);
        std::printf("\n");
        print(e.children, depth + 1, items, icons, submenus);
    }
}

}  // namespace

int wmain() {
    const HWND foreground_before = GetForegroundWindow();
    check(SUCCEEDED(CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED)), "CoInitializeEx STA");

    // A real file for the handler to look at, in a folder of our own.
    wchar_t temp[MAX_PATH]{};
    GetTempPathW(MAX_PATH, temp);
    const std::wstring folder = std::wstring(temp) + L"bb-s3-" + std::to_wstring(GetCurrentProcessId());
    CreateDirectoryW(folder.c_str(), nullptr);
    const std::wstring file = folder + L"\\sample.txt";
    {
        const HANDLE h = CreateFileW(file.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, 0, nullptr);
        check(h != INVALID_HANDLE_VALUE, "create sample file");
        DWORD written = 0;
        WriteFile(h, "hello", 5, &written, nullptr);
        CloseHandle(h);
    }

    // The icon path, on a bitmap of our own, since a handler may have its icons switched off: a
    // 16x16 32-bit DIB with the left half opaque must read back as exactly 128 opaque pixels.
    {
        BITMAPINFO info{};
        info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
        info.bmiHeader.biWidth = 16;
        info.bmiHeader.biHeight = -16;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        void* bits = nullptr;
        const HBITMAP dib = CreateDIBSection(nullptr, &info, DIB_RGB_COLORS, &bits, nullptr, 0);
        check(dib != nullptr && bits != nullptr, "CreateDIBSection");
        auto* px = static_cast<BYTE*>(bits);
        for (int y = 0; y < 16; ++y) {
            for (int x = 0; x < 16; ++x) {
                BYTE* p = px + (y * 16 + x) * 4;
                p[0] = p[1] = p[2] = x < 8 ? 200 : 0;
                p[3] = x < 8 ? 255 : 0;
            }
        }
        Entry probe;
        read_icon(dib, probe);
        check(probe.icon_w == 16 && probe.icon_h == 16, "synthetic icon size read back");
        check(probe.icon_opaque == 128, "synthetic icon alpha survives GetDIBits");
        DeleteObject(dib);
    }

    std::wstring name;
    CLSID clsid{};
    check(find_handler(name, clsid), "a context-menu handler is registered under HKCR\\*");
    std::printf("S3 handler: %ls\n", name.c_str());

    size_t items = 0, icons = 0, submenus = 0;
    {
        Selection sel;
        check(sel.create(folder, {file}), "folder PIDL + IDataObject for the selection");
        check(sel.data != nullptr, "the selection has a data object");

        Host host;
        check(host.create(clsid, sel), "CoCreateInstance + IShellExtInit::Initialize");
        if (host.menu) {
            const auto entries = host.query();
            check(!entries.empty(), "QueryContextMenu added items");
            print(entries, 0, items, icons, submenus);
            std::printf("S3 walked %zu items, %zu submenus, %zu with icons\n", items, submenus, icons);
            check(items >= 2, "the handler contributed more than one item");
            if (name == L"7-Zip") {
                // 7-Zip offers at least Open archive / Extract / Add in one layout or another.
                check(items >= 3, "7-Zip offered its usual items");
            }
        }
        // The host and selection are released here, after the walk, as the session does after Closed.
    }

    DeleteFileW(file.c_str());
    RemoveDirectoryW(folder.c_str());
    CoUninitialize();
    check(GetForegroundWindow() == foreground_before, "foreground untouched");

    if (failures) {
        std::printf("S3 FAILED: %d check(s)\n", failures);
        return 1;
    }
    std::printf("S3 OK: hosted %ls in a private HMENU, walked it with WM_INITMENUPOPUP by hand, read icons via GetDIBits, invoked nothing\n",
                name.c_str());
    return 0;
}
