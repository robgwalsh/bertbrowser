// The product's shell-menu source: registry enumeration and COM handlers hosted one at a time in a private
// menu that is never shown (spikes/s3_shellmenu proved the approach). Everything runs on the UI thread, because
// the extensions are apartment-threaded. Nothing is invoked here except by `ShellMenuSession::invoke`, which only
// the product's menu calls - a scripted run uses the canned source and loads no foreign code.
#include "views/shell_menu.h"

#include <shellapi.h>
#include <shlobj.h>
#include <shobjidl.h>
#include <wrl/client.h>

#include "views/main_view.h"
#include "views/ops.h"

#include <algorithm>
#include <map>

namespace bb::views {

using Microsoft::WRL::ComPtr;

namespace {

namespace reg {

std::vector<std::wstring> subkeys(const std::wstring& path) {
    std::vector<std::wstring> out;
    HKEY key{};
    if (RegOpenKeyExW(HKEY_CLASSES_ROOT, path.c_str(), 0, KEY_READ, &key) != ERROR_SUCCESS) return out;
    for (DWORD i = 0;; ++i) {
        wchar_t name[256];
        DWORD len = 256;
        if (RegEnumKeyExW(key, i, name, &len, nullptr, nullptr, nullptr, nullptr) != ERROR_SUCCESS) break;
        out.emplace_back(name, len);
    }
    RegCloseKey(key);
    return out;
}

// A string value (the default when `name` is empty); empty when absent or not text.
std::wstring text(const std::wstring& path, const wchar_t* name = nullptr) {
    wchar_t buffer[1024];
    DWORD bytes = sizeof(buffer) - sizeof(wchar_t), type = 0;
    const LSTATUS st = RegGetValueW(HKEY_CLASSES_ROOT, path.c_str(), name, RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND, &type,
                                    buffer, &bytes);
    return st == ERROR_SUCCESS ? std::wstring(buffer) : std::wstring();
}

bool has_value(const std::wstring& path, const wchar_t* name) {
    return RegGetValueW(HKEY_CLASSES_ROOT, path.c_str(), name, RRF_RT_ANY, nullptr, nullptr, nullptr) == ERROR_SUCCESS;
}

}  // namespace reg

struct Raw {
    std::string handlers = "[", verbs = "[";
    bool first_handler = true, first_verb = true;
    // The command each verb runs, for the family that registered it.
    std::map<std::string, std::wstring> command_of;
    std::map<std::string, std::wstring> name_of;
};

std::string esc(const std::wstring& s) { return ops::q(s); }
std::string flag(bool b) { return b ? "true" : "false"; }

void scan_family(const std::wstring& family, Raw& raw) {
    const std::wstring handlers = family + L"\\shellex\\ContextMenuHandlers";
    for (const std::wstring& name : reg::subkeys(handlers)) {
        std::wstring value = reg::text(handlers + L"\\" + name);
        if (value.empty()) value = name;
        CLSID clsid{};
        if (FAILED(CLSIDFromString(value.c_str(), &clsid))) continue;
        wchar_t braced[64];
        StringFromGUID2(clsid, braced, 64);
        const std::wstring class_name = reg::text(std::wstring(L"CLSID\\") + braced);
        raw.handlers += std::string(raw.first_handler ? "" : ",") + "{\"family\":" + esc(family) + ",\"name\":" + esc(name) + ",\"clsid\":" +
                        esc(braced) + (class_name.empty() ? "" : ",\"className\":" + esc(class_name)) + "}";
        raw.first_handler = false;
    }
    const std::wstring shell = family + L"\\shell";
    for (const std::wstring& verb : reg::subkeys(shell)) {
        const std::wstring key = shell + L"\\" + verb;
        std::wstring display = reg::text(key, L"MUIVerb");
        if (display.empty()) display = reg::text(key);
        if (!display.empty() && display[0] == L'@') display.clear();  // a resource string: the verb names itself
        const std::wstring command = reg::text(key + L"\\command");
        const bool delegate = reg::has_value(key + L"\\command", L"DelegateExecute");
        const bool sub = reg::has_value(key, L"SubCommands") || reg::has_value(key, L"ExtendedSubCommandsKey");
        const std::string lowered = [&] {
            std::string v = core::utf8(verb);
            std::transform(v.begin(), v.end(), v.begin(), [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
            return v;
        }();
        raw.verbs += std::string(raw.first_verb ? "" : ",") + "{\"family\":" + esc(family) + ",\"verb\":" + esc(verb) +
                     (display.empty() ? "" : ",\"displayName\":" + esc(display)) + (command.empty() ? "" : ",\"command\":" + esc(command)) +
                     ",\"extended\":" + flag(reg::has_value(key, L"Extended")) + ",\"legacyDisable\":" + flag(reg::has_value(key, L"LegacyDisable")) +
                     ",\"programmaticOnly\":" + flag(reg::has_value(key, L"ProgrammaticAccessOnly")) +
                     ",\"hasDelegateExecute\":" + flag(delegate) + ",\"hasSubCommands\":" + flag(sub) + "}";
        raw.first_verb = false;
        if (!command.empty() && !raw.command_of.count(lowered)) raw.command_of[lowered] = command;
        if (!display.empty() && !raw.name_of.count(lowered)) raw.name_of[lowered] = display;
    }
}

constexpr UINT kIdFirst = 1, kIdLast = 0x7FFF;

// One COM handler and the private menu it filled.
struct Host {
    ComPtr<IContextMenu> menu;
    ComPtr<IContextMenu2> menu2;
    ComPtr<IContextMenu3> menu3;
    HMENU hmenu = nullptr;
    ~Host() {
        if (hmenu) DestroyMenu(hmenu);
    }
    void message(UINT msg, WPARAM wp, LPARAM lp) {
        if (menu3) {
            LRESULT r = 0;
            menu3->HandleMenuMsg2(msg, wp, lp, &r);
        } else if (menu2) {
            menu2->HandleMenuMsg(msg, wp, lp);
        }
    }
    std::wstring verb_of(UINT id) {
        wchar_t buffer[256]{};
        if (menu->GetCommandString(id - kIdFirst, GCS_VERBW, nullptr, reinterpret_cast<CHAR*>(buffer), 256) == S_OK) return buffer;
        return {};
    }
};

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
            if (FAILED(SHCreateShellItemArrayFromIDLists(static_cast<UINT>(raw.size()), raw.data(), &array))) return false;
            if (FAILED(array->BindToHandler(nullptr, BHID_DataObject, IID_PPV_ARGS(&data)))) return false;
        }
        return true;
    }
};

class RegistrySession : public ShellMenuSession {
public:
    RegistrySession(MainView& main) : main_(main) {}

    std::wstring invoke(int command, HWND owner) override {
        if (command < 0 || static_cast<size_t>(command) >= commands_.size()) return {};
        const Command& c = commands_[static_cast<size_t>(command)];
        if (c.host) {
            CMINVOKECOMMANDINFOEX info{};
            info.cbSize = sizeof(info);
            info.fMask = CMIC_MASK_UNICODE;
            info.hwnd = owner;
            info.lpVerb = MAKEINTRESOURCEA(c.id - kIdFirst);
            info.lpVerbW = MAKEINTRESOURCEW(c.id - kIdFirst);
            info.nShow = SW_SHOWNORMAL;
            const HRESULT hr = c.host->menu->InvokeCommand(reinterpret_cast<CMINVOKECOMMANDINFO*>(&info));
            return SUCCEEDED(hr) ? std::wstring() : L"That entry could not be run.";
        }
        ops::run(main_.core(), "shell-verb",
                 "{\"command\":" + ops::q(c.verb_command) + ",\"target\":" + ops::q(c.target) + ",\"workingDirectory\":" + ops::q(c.work) + "}");
        return {};
    }

    struct Command {
        Host* host = nullptr;
        UINT id = 0;
        std::wstring verb_command, target, work;
    };

    std::vector<std::unique_ptr<Host>> hosts_;
    std::vector<Command> commands_;
    std::unique_ptr<Selection> selection_;

    // Walks a menu the handler filled, delivering WM_INITMENUPOPUP by hand before each submenu.
    std::vector<ShellEntry> walk(Host& host, HMENU m) {
        std::vector<ShellEntry> out;
        const int count = GetMenuItemCount(m);
        for (int pos = 0; pos < count; ++pos) {
            MENUITEMINFOW info{};
            info.cbSize = sizeof(info);
            info.fMask = MIIM_STATE | MIIM_ID | MIIM_SUBMENU | MIIM_STRING | MIIM_FTYPE;
            if (!GetMenuItemInfoW(m, static_cast<UINT>(pos), TRUE, &info)) continue;
            ShellEntry e;
            if (info.fType & MFT_SEPARATOR) {
                e.separator = true;
                out.push_back(e);
                continue;
            }
            std::wstring text;
            if (!(info.fType & (MFT_BITMAP | MFT_OWNERDRAW)) && info.cch > 0) {
                text.assign(info.cch + 1, L'\0');
                MENUITEMINFOW t{};
                t.cbSize = sizeof(t);
                t.fMask = MIIM_STRING;
                t.dwTypeData = text.data();
                t.cch = info.cch + 1;
                if (GetMenuItemInfoW(m, static_cast<UINT>(pos), TRUE, &t)) text.resize(t.cch);
                else text.clear();
            }
            e.enabled = (info.fState & (MFS_DISABLED | MFS_GRAYED)) == 0;
            if (info.hSubMenu) {
                host.message(WM_INITMENUPOPUP, reinterpret_cast<WPARAM>(info.hSubMenu), static_cast<LPARAM>(pos));
                e.children = walk(host, info.hSubMenu);
            } else {
                if ((info.fType & MFT_OWNERDRAW) && text.empty()) text = host.verb_of(info.wID);
                e.command = static_cast<int>(commands_.size());
                Command c;
                c.host = &host;
                c.id = info.wID;
                commands_.push_back(c);
            }
            const size_t tab = text.find(L'\t');
            if (tab != std::wstring::npos) {
                e.gesture = text.substr(tab + 1);
                text.resize(tab);
            }
            e.header = text;
            out.push_back(std::move(e));
        }
        return out;
    }

private:
    MainView& main_;
};

// Separators at the ends and doubled ones go, and a submenu that came back empty goes with them.
std::vector<ShellEntry> tidy(std::vector<ShellEntry> in) {
    std::vector<ShellEntry> out;
    for (ShellEntry& e : in) {
        if (!e.separator && e.header.empty() && e.children.empty()) continue;
        if (!e.children.empty()) e.children = tidy(std::move(e.children));
        if (e.separator && (out.empty() || out.back().separator)) continue;
        out.push_back(std::move(e));
    }
    while (!out.empty() && out.back().separator) out.pop_back();
    return out;
}

[[maybe_unused]] static std::wstring capitalised(std::wstring s) {
    if (!s.empty()) s[0] = static_cast<wchar_t>(towupper(s[0]));
    return s;
}

class RegistrySource : public ShellMenuSource {
public:
    explicit RegistrySource(MainView& main) : main_(main) {}

    std::vector<ShellExtensionInfo> catalog() override {
        Raw raw;
        for (const wchar_t* family : {L"*", L"AllFilesystemObjects", L"Directory", L"Folder", L"Drive", L"Directory\\Background"}) scan_family(family, raw);
        std::vector<ShellExtensionInfo> out;
        const json::Value cat = ops::query(main_.core(), "shell-catalog", "{\"handlers\":" + raw.handlers + "],\"verbs\":" + raw.verbs + "]}");
        for (size_t i = 0; i < cat.size(); ++i) {
            out.push_back({cat[i]["id"].str(), core::utf8(core::wide(cat[i]["name"].str())), cat[i]["kind"].str() == "handler"});
            out.back().name = cat[i]["name"].str();
        }
        return out;
    }

    std::unique_ptr<ShellMenuSession> open(const std::vector<ShellTarget>& targets, ShellContext context, const std::wstring& folder) override {
        // The type of the first item decides the type-specific keys, as it does in Explorer.
        std::wstring prog_id, perceived;
        if (context == ShellContext::Items && !targets.empty() && !targets[0].is_directory) {
            const std::wstring& p = targets[0].path;
            const size_t dot = p.find_last_of(L'.');
            if (dot != std::wstring::npos && p.find_last_of(L"\\/") < dot) {
                const std::wstring ext = p.substr(dot);
                prog_id = reg::text(ext);
                perceived = reg::text(ext, L"PerceivedType");
            }
        }
        std::string t = "[";
        for (size_t i = 0; i < targets.size(); ++i) {
            t += std::string(i ? "," : "") + "{\"path\":" + ops::q(targets[i].path) + ",\"isDirectory\":" + (targets[i].is_directory ? "true" : "false") + "}";
        }
        t += "]";
        const json::Value keys = ops::query(main_.core(), "shell-keys",
                                            "{\"targets\":" + t + ",\"context\":\"" + (context == ShellContext::Background ? "background" : "items") +
                                                "\",\"fileType\":{\"progId\":" + ops::q(prog_id) + ",\"perceived\":" + ops::q(perceived) + "}}");
        Raw raw;
        for (size_t i = 0; i < keys.size(); ++i) scan_family(core::wide(keys[i].str()), raw);
        const json::Value cat = ops::query(main_.core(), "shell-catalog", "{\"handlers\":" + raw.handlers + "],\"verbs\":" + raw.verbs + "]}");

        auto session = std::make_unique<RegistrySession>(main_);
        session->selection_ = std::make_unique<Selection>();
        std::vector<std::wstring> paths;
        for (const auto& tg : targets) paths.push_back(tg.path);
        const bool have_selection =
            session->selection_->create(context == ShellContext::Background && !targets.empty() ? targets[0].path : folder, context == ShellContext::Background ? std::vector<std::wstring>{} : paths);

        for (size_t i = 0; i < cat.size(); ++i) {
            const json::Value& e = cat[i];
            ShellGroup group;
            group.extension_id = e["id"].str();
            if (e["kind"].str() == "handler") {
                if (!have_selection) continue;
                CLSID clsid{};
                if (FAILED(CLSIDFromString(core::wide(e["clsid"].str()).c_str(), &clsid))) continue;
                auto host = std::make_unique<Host>();
                ComPtr<IUnknown> unk;
                if (FAILED(CoCreateInstance(clsid, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&unk)))) continue;
                ComPtr<IShellExtInit> init;
                if (FAILED(unk.As(&init)) || FAILED(unk.As(&host->menu))) continue;
                if (FAILED(init->Initialize(session->selection_->folder, session->selection_->data.Get(), nullptr))) continue;
                unk.As(&host->menu2);
                unk.As(&host->menu3);
                host->hmenu = CreatePopupMenu();
                if (FAILED(host->menu->QueryContextMenu(host->hmenu, 0, kIdFirst, kIdLast, CMF_NORMAL))) continue;
                group.entries = tidy(session->walk(*host, host->hmenu));
                session->hosts_.push_back(std::move(host));
            } else {
                // A registry verb: its own line, run through the one launcher.
                const std::string id = e["id"].str();
                std::string verb = id.substr(id.find(':') + 1);
                ShellEntry entry;
                entry.header = core::wide(e["name"].str());
                entry.command = static_cast<int>(session->commands_.size());
                RegistrySession::Command c;
                const auto it = raw.command_of.find(verb);
                if (it != raw.command_of.end()) c.verb_command = it->second;
                if (!targets.empty()) {
                    c.target = targets[0].path;
                    c.work = context == ShellContext::Background || targets[0].is_directory
                                 ? targets[0].path
                                 : targets[0].path.substr(0, targets[0].path.find_last_of(L"\\/"));
                } else {
                    c.target = c.work = folder;
                }
                session->commands_.push_back(c);
                group.entries.push_back(std::move(entry));
            }
            if (!group.entries.empty()) session->groups.push_back(std::move(group));
        }
        return session;
    }

private:
    MainView& main_;
};

}  // namespace

std::unique_ptr<ShellMenuSource> make_registry_shell_source(MainView& main) { return std::make_unique<RegistrySource>(main); }

}  // namespace bb::views
