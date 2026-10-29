// The shell menu a scripted run sees: four extensions, canned, so no foreign code is loaded and nothing is
// launched (tools/BertBrowser.Harness/CannedShellMenuSource.cs). What a person's machine adds to the menu
// differs from one to the next; a screenshot must not.
#pragma once
#include "views/shell_menu.h"

namespace bbh {

class CannedShellMenuSession : public bb::views::ShellMenuSession {
public:
    std::wstring invoke(int command, HWND) override {
        if (command >= 0 && static_cast<size_t>(command) < headers.size()) invocations.push_back(headers[static_cast<size_t>(command)]);
        return L"The harness does not run shell extensions: it was not invoked.";
    }
    std::vector<std::wstring> headers, invocations;
};

class CannedShellMenuSource : public bb::views::ShellMenuSource {
public:
    std::vector<bb::views::ShellExtensionInfo> catalog() override {
        return {{"verb:git_shell", "Open Git Bash here", false},
                {"verb:vscode", "Open with Code", false},
                {"clsid:{23170f69-40c1-278a-1000-000100020000}", "7-Zip", true},
                {"clsid:{30351346-7b7d-4fcc-81b4-1e394ca267eb}", "TortoiseSVN", true}};
    }

    std::unique_ptr<bb::views::ShellMenuSession> open(const std::vector<bb::views::ShellTarget>& targets, bb::views::ShellContext context,
                                                      const std::wstring&) override {
        using bb::views::ShellEntry;
        auto session = std::make_unique<CannedShellMenuSession>();
        auto item = [&](const wchar_t* header, bool enabled = true) {
            ShellEntry e;
            e.header = header;
            e.enabled = enabled;
            e.command = static_cast<int>(session->headers.size());
            session->headers.push_back(header);
            return e;
        };
        auto sep = [] {
            ShellEntry e;
            e.separator = true;
            return e;
        };
        auto group = [&](const char* id, std::vector<ShellEntry> entries) {
            session->groups.push_back({id, std::move(entries)});
        };
        // One line carries a picture of its own, as an extension's HBITMAP would: a flat blue square.
        auto swatch = [] {
            auto img = std::make_shared<bb::ui::ThumbImage>();
            img->width = img->height = 16;
            img->bgra.resize(16 * 16 * 4);
            for (size_t i = 0; i < img->bgra.size(); i += 4) {
                img->bgra[i] = 0xD0;
                img->bgra[i + 1] = 0x78;
                img->bgra[i + 2] = 0x00;
                img->bgra[i + 3] = 0xFF;
            }
            return bb::ui::ThumbPtr(img);
        };
        ShellEntry code = item(L"Open with Code");
        code.picture = swatch();
        group("verb:git_shell", {item(L"Open Git Bash here")});
        group("verb:vscode", {std::move(code)});
        if (context == bb::views::ShellContext::Items) {
            bool any_file = false;
            for (const auto& t : targets) any_file = any_file || !t.is_directory;
            ShellEntry zip = item(L"7-Zip");
            zip.command = -1;
            zip.children = {item(L"Add to archive..."), item(L"Extract Here", any_file), sep(), item(L"Test archive", false)};
            group("clsid:{23170f69-40c1-278a-1000-000100020000}", {std::move(zip)});
        }
        ShellEntry svn = item(L"TortoiseSVN");
        svn.command = -1;
        svn.children = {item(L"SVN Checkout..."), sep(), item(L"Settings")};
        group("clsid:{30351346-7b7d-4fcc-81b4-1e394ca267eb}", {std::move(svn)});
        return session;
    }
};

}  // namespace bbh
