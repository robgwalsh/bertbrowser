#include "views/tools/tool_dialogs.h"

#include "views/dialogs/dialogs.h"
#include "views/dialogs/file_pickers.h"
#include "views/dialogs/forms.h"
#include "views/main/main_view.h"
#include "views/bridge/model.h"
#include "views/dialogs/ops_card.h"
#include "views/tools/tool_state.h"
#include "views/tools/tool_window.h"
#include "views/tools/tool_widgets.h"

#include <windows.h>

#include <algorithm>
#include <cwctype>
#include <map>
#include <set>

namespace bb::views {

using namespace bb::ui;
namespace f = forms;
namespace tw = tools;

namespace {

// ---- the card every dialog is -------------------------------------------------------------------------

struct Card {
    std::unique_ptr<Panel> root;
    StackPanel* body = nullptr;
    StackPanel* buttons = nullptr;
};

Card make_card(const std::wstring& title, float width) {
    Card c;
    c.root = std::make_unique<Panel>();
    c.root->pad = 1;
    c.root->width = width;
    c.root->style.bg = tok::OverlayBorder;
    auto* dock = c.root->emplace<DockPanel>();
    dock->style.bg = tok::OverlayBackground;
    auto* head = dock->emplace<Panel>();
    head->name = "CardHeader";  // a tool window shows the title in its own caption instead
    head->dock = Dock::Top;
    head->pad = 14;
    auto* t = head->emplace<TextBlock>(title);
    t->style.font.size = 16.0f;
    t->style.font.weight = 600;
    auto* foot = dock->emplace<StackPanel>(Orientation::Horizontal, 8.0f);
    foot->dock = Dock::Bottom;
    foot->pad = 12;
    foot->style.bg = tok::SurfaceBackground;
    foot->emplace<Widget>()->flex = 1;
    c.buttons = foot;
    c.body = dock->emplace<StackPanel>(Orientation::Vertical, 8.0f);
    c.body->dock = Dock::Fill;
    c.body->pad = 16;
    return c;
}

Button* add_button(Card& c, const wchar_t* label, std::function<void()> fn, bool primary = false, float width = 104.0f) {
    auto* b = f::button(*c.buttons, label, std::move(fn), width);
    b->primary = primary;
    return b;
}

TextBlock* mono(Widget& parent, const std::wstring& t, Token fg = tok::TextPrimary, float size = 12.5f) {
    auto* tb = f::text(parent, t, fg, size);
    tb->style.font.family = gfx::Family::Mono;
    return tb;
}

std::wstring W(const json::Value& v) { return core::wide(v.str()); }

std::wstring parent_of(const std::wstring& path) {
    const size_t cut = path.find_last_of(L"\\/");
    if (cut == std::wstring::npos) return {};
    return cut <= 2 && path.size() > 1 && path[1] == L':' ? path.substr(0, cut + 1) : path.substr(0, cut);
}

// A card that says why the window could not open.
std::wstring bb_file_name(const std::wstring& path) {
    const size_t cut = path.find_last_of(L"\\/");
    return cut == std::wstring::npos ? path : path.substr(cut + 1);
}

// The shell's icon for a row's file, as the folder list shows it. Spec: the tool windows' FileIcon columns.
std::function<ThumbPtr(uint32_t)> file_picture(MainView& main, const std::wstring& path, bool directory) {
    ShellIcons* icons = &main.shell_icons();
    return [icons, path, directory](uint32_t px) {
        return icons->file(path, directory, directory ? FILE_ATTRIBUTE_DIRECTORY : 0, px);
    };
}

std::unique_ptr<Panel> refusal(DialogContext& cx, const std::wstring& title, const std::wstring& why) {
    Card c = make_card(title, 460);
    f::text(*c.body, why, tok::TextSecondary, 13.0f)->wrap = true;
    f::button(*c.buttons, L"Close", cx.close, 104.0f)->primary = true;
    return std::move(c.root);
}

std::vector<SelectedRow> selected_files(MainView& main) {
    std::vector<SelectedRow> out;
    for (SelectedRow& r : main.selected_rows()) {
        if (!r.dir) out.push_back(std::move(r));
    }
    return out;
}

// ---- checksums -------------------------------------------------------------------------------------------

std::wstring verify_words(const std::string& state) {
    if (state == "Ok") return L"OK";
    if (state == "Mismatch") return L"Does not match";
    if (state == "Missing") return L"Missing";
    if (state == "Unreadable") return L"Could not be read";
    if (state == "Refused") return L"Not read";
    if (state == "NotListed") return L"Not listed";
    return core::wide(state);
}

Token verify_colour(const std::string& state) {
    if (state == "Ok") return tok::TextLink;
    if (state == "NotListed") return tok::TextMuted;
    return tok::ErrorForeground;
}

std::unique_ptr<Panel> checksum_card(DialogContext& cx, const json::Value& m) {
    Card c = make_card(W(m["title"]), 780);
    const bool verify = m["mode"].str() == "verify";
    if (!m["banner"].is_null()) {
        f::text(*c.body, W(m["banner"]), tok::WarningForeground, 13.0f)->wrap = true;
    }

    // The algorithms: all five read in one pass, so ticking another only reads what is missing.
    struct Choice {
        std::string id;
        bool on;
    };
    std::vector<Choice> choices;
    for (size_t i = 0; i < m["algorithms"].size(); ++i) {
        choices.push_back({m["algorithms"][i]["id"].str(), m["algorithms"][i]["selected"].boolean()});
    }
    auto* algs = f::row(*c.body, 16);
    for (size_t i = 0; i < choices.size(); ++i) {
        const json::Value& a = m["algorithms"][i];
        auto* box = f::check(*algs, W(a["label"]), choices[i].on, [cx, choices, i](bool on) mutable {
            std::vector<std::wstring> ids;
            for (size_t k = 0; k < choices.size(); ++k) {
                if (k == i ? on : choices[k].on) ids.push_back(core::wide(choices[k].id));
            }
            try_tool_call(cx.main, "checksum.algorithms", "{\"algorithms\":" + json_array(ids) + "}");
            cx.main.open_dialog(L"checksum-show");
        });
        box->name = "Algorithm" + a["id"].str();
        if (!a["note"].is_null()) box->tooltip = W(a["note"]);
        box->set_enabled(!verify);
    }

    // Reading happens off the UI thread; the window says how far it has got, and can be stopped.
    const bool running = m["running"].boolean();
    if (running) {
        const json::Value& p = m["progress"];
        auto* line = f::row(*c.body, 10);
        auto* head = f::text(*line, W(p["headline"]), tok::TextSecondary, 13.0f);
        head->name = "ChecksumProgress";
        head->flex = 1;
        f::text(*line, W(p["detail"]), tok::TextMuted, 12.5f);
        f::button(*line, L"Stop", [cx] {
            try_tool_call(cx.main, "checksum.stop");
            cx.main.open_dialog(L"checksum-show");
        }, 80)->name = "ChecksumStop";
        auto* bar = c.body->emplace<ProgressBar>();
        bar->height = 6;
        // Negative: no byte total yet, so the bar only says that something is happening.
        if (p["fraction"].num() < 0) bar->set_indeterminate(true);
        else bar->set_value(p["fraction"].num());
    }
    if (!m["stopped"].is_null()) f::text(*c.body, W(m["stopped"]), tok::WarningForeground, 13.0f)->name = "ChecksumStopped";

    if (!verify && m["canCompare"].boolean()) {
        auto* paste = f::row(*c.body, 8);
        f::text(*paste, L"Compare with", tok::TextSecondary, 13.0f)->width = 96;
        const size_t before = std::count_if(choices.begin(), choices.end(), [](const Choice& x) { return x.on; });
        auto* edit = f::field(*paste, W(m["expected"]), 400, nullptr, L"Paste a checksum, or a whole line of a checksum file");
        edit->name = "ExpectedDigest";
        auto* verdict = f::text(*paste, W(m["pasteVerdict"]), m["pasteMatched"].boolean() ? tok::TextLink : tok::ErrorForeground, 13.0f);
        verdict->name = "PasteVerdict";
        verdict->width = 170;
        edit->on_changed = [cx, verdict, before](const std::wstring& text) {
            const json::Value next = try_tool_call(cx.main, "checksum.expected", "{\"text\":" + json_quote(text) + "}");
            if (next.is_null()) return;
            size_t now = 0;
            for (size_t i = 0; i < next["algorithms"].size(); ++i) now += next["algorithms"][i]["selected"].boolean() ? 1 : 0;
            verdict->set_text(W(next["pasteVerdict"]));
            verdict->style.fg = next["pasteMatched"].boolean() ? tok::TextLink : tok::ErrorForeground;
            verdict->invalidate();
            // The paste named an algorithm that was not ticked: it was ticked and computed.
            if (now != before) cx.main.open_dialog(L"checksum-show");
        };
    }

    auto* scroll = c.body->emplace<ScrollViewer>();
    scroll->height = 300;
    scroll->style.bg = tok::InputBackground;
    scroll->style.border = tok::BorderSubtle;
    auto* list = scroll->emplace<StackPanel>(Orientation::Vertical, 10.0f);
    list->pad = 10;
    list->name = "ChecksumRows";
    if (m["rows"].size() == 0) {
        f::text(*list, W(m["emptyMessage"]), tok::TextMuted, 13.0f);
    }
    for (size_t i = 0; i < m["rows"].size(); ++i) {
        const json::Value& row = m["rows"][i];
        auto* block = f::column(*list, 2);
        block->name = "ChecksumRow:" + row["name"].str();
        auto* head = f::row(*block, 8);
        head->emplace<f::Picture>(file_picture(cx.main, W(row["path"]), false))->width = 16;
        f::text(*head, W(row["name"]), tok::TextPrimary, 14.0f, 600)->vcenter = true;
        if (!row["verifyState"].is_null()) {
            f::text(*head, verify_words(row["verifyState"].str()), verify_colour(row["verifyState"].str()), 13.0f, 600);
        }
        if (!row["verifyDetail"].is_null()) f::text(*block, W(row["verifyDetail"]), tok::TextMuted, 12.5f);
        if (!row["error"].is_null()) f::text(*block, W(row["error"]), tok::ErrorForeground, 12.5f)->wrap = true;
        for (size_t d = 0; d < row["digests"].size(); ++d) {
            const json::Value& dg = row["digests"][d];
            auto* line = f::row(*block, 10);
            f::text(*line, W(dg["label"]), tok::TextSecondary, 12.5f)->width = 64;
            auto* digest = mono(*line, W(dg["digest"]), dg["pasteMatch"].boolean() ? tok::TextLink : tok::TextPrimary, 12.0f);
            digest->wrap = true;
            digest->flex = 1;
            const std::wstring text = W(dg["digest"]);
            auto* copy = f::button(*line, L"Copy", [cx, text] {
                if (Widget* any = cx.main.dialog_widget("ChecksumRows")) {
                    if (any->root()) any->root()->clipboard.write(text);
                }
                bb_status_set(cx.main.core().app(), core::s(L"Copied the checksum"));
            }, 64);
            copy->name = "CopyDigest:" + row["name"].str() + ":" + dg["algorithm"].str();
        }
    }

    if (!m["verifySummary"].is_null()) {
        auto* s = f::text(*c.body, W(m["verifySummary"]), tok::TextSecondary, 13.0f);
        s->name = "VerifySummary";
    }
    const std::wstring& note = tools_ui(cx.main).checksum_note;
    if (!note.empty()) f::text(*c.body, note, tok::TextMuted, 12.5f)->wrap = true;

    Widget* anchor = c.root.get();
    std::wstring all;
    std::wstring folder, first_algorithm;
    for (size_t i = 0; i < m["rows"].size(); ++i) {
        const json::Value& row = m["rows"][i];
        if (folder.empty()) folder = parent_of(W(row["path"]));
        // Lowercase whatever the algorithm shows, as Copy all always has. Spec: ChecksumViewModel.CopyAll.
        for (size_t d = 0; d < row["digests"].size(); ++d) {
            std::wstring digest = W(row["digests"][d]["digest"]);
            for (wchar_t& ch : digest) ch = static_cast<wchar_t>(towlower(ch));
            all += digest + L"  " + W(row["name"]) + L"\n";
        }
    }
    std::wstring extension;
    for (size_t i = 0; i < m["algorithms"].size(); ++i) {
        if (m["algorithms"][i]["selected"].boolean()) {
            first_algorithm = W(m["algorithms"][i]["id"]);
            extension = W(m["algorithms"][i]["extension"]);
            break;
        }
    }
    {
        // In Verify too: what was read can be saved, and read again. Spec: ChecksumWindow.xaml.
        // Windows' Save as, starting beside the files with the first ticked algorithm's extension, rather
        // than writing a fixed name over whatever is there. Spec: ChecksumWindow.Save_Click.
        std::vector<std::pair<std::wstring, std::wstring>> filters;
        unsigned at = 0;
        std::vector<std::wstring> ids;
        for (size_t i = 0; i < m["algorithms"].size(); ++i) {
            const json::Value& a = m["algorithms"][i];
            filters.push_back({W(a["label"]) + L" (*" + W(a["extension"]) + L")", L"*" + W(a["extension"])});
            ids.push_back(W(a["id"]));
            if (W(a["id"]) == first_algorithm) at = static_cast<unsigned>(i);
        }
        const std::wstring suggested = m["rows"].size() == 1 ? W(m["rows"][0]["name"]) : bb_file_name(folder);
        auto* save = add_button(c, L"Save\u2026", [cx, folder, extension, filters, ids, at, suggested] {
            if (folder.empty() || ids.empty() || !cx.main.shows_dialogs()) return;
            PickPath pick;
            pick.title = L"Save checksum file";
            pick.folder = folder;
            pick.file_name = suggested + extension;
            pick.default_ext = extension.size() > 1 ? extension.substr(1) : L"";
            pick.filters = filters;
            pick.filter_index = at;
            ToolWindow* w = cx.main.tool_window(L"checksum");
            const auto path = pick_save_file(w ? w->window().hwnd() : cx.main.window().hwnd(), pick);
            if (!path) return;
            // The algorithm is the one the chosen name's extension says, so a .md5 is written as MD5.
            std::wstring algorithm = ids[at];
            const json::Value of = try_tool_call(cx.main, "checksum.algorithm-of", "{\"path\":" + json_quote(*path) + "}");
            if (!of["algorithm"].is_null()) algorithm = W(of["algorithm"]);
            const json::Value r = try_tool_call(cx.main, "checksum.save", "{\"path\":" + json_quote(*path) + ",\"algorithm\":" + json_quote(algorithm) + "}");
            tools_ui(cx.main).checksum_note = r["ok"].boolean() ? (r["note"].is_null() ? L"Saved " + bb_file_name(*path) + L"." : W(r["note"])) : W(r["error"]);
            cx.main.open_dialog(L"checksum-show");
        });
        save->name = "ChecksumSave";
        save->set_enabled(m["canSave"].boolean());
        auto* again = add_button(c, L"Hash again", [cx] {
            try_tool_call(cx.main, "checksum.rehash");
            cx.main.open_dialog(L"checksum-show");
        }, false, 120);
        again->name = "ChecksumRehash";
        again->set_enabled(!running && m["rows"].size() > 0);
    }
    add_button(c, L"Copy all", [anchor, all] {
        if (!all.empty() && anchor->root()) anchor->root()->clipboard.write(all);
    })->set_enabled(!all.empty());
    add_button(c, L"Close", cx.close, true);
    return std::move(c.root);
}

std::unique_ptr<Panel> checksum_start(DialogContext& cx, bool verify) {
    tools_ui(cx.main).checksum_note.clear();
    const std::vector<SelectedRow> files = selected_files(cx.main);
    auto is_checksum_file = [&](const std::wstring& path) {
        return !try_tool_call(cx.main, "checksum.algorithm-of", "{\"path\":" + json_quote(path) + "}")["algorithm"].is_null();
    };
    // A single checksum file opens in Verify: being shown a .sha256's own digest is not what anybody meant.
    if (!verify && files.size() == 1 && is_checksum_file(files[0].path)) verify = true;
    std::wstring against;
    if (verify) {
        if (files.size() == 1 && is_checksum_file(files[0].path)) {
            against = files[0].path;
        } else if (cx.main.shows_dialogs()) {
            // Verify is not about the selection: it asks which checksum file to check against.
            // Spec: DirectoryTabView.VerifyChecksums.
            PickPath pick;
            pick.title = L"Verify against a checksum file";
            pick.folder = cx.main.core().tab_text(cx.main.active_tab(), BB_TEXT_PATH);
            pick.filters = {{L"Checksum files", L"*.sfv;*.md5;*.sha1;*.sha256;*.sha512"}, {L"All files (*.*)", L"*.*"}};
            ToolWindow* w = cx.main.tool_window(L"checksum");
            if (const auto chosen = pick_open_file(w ? w->window().hwnd() : cx.main.window().hwnd(), pick)) against = *chosen;
        }
        if (against.empty()) return nullptr;  // nothing chosen: no window
    } else if (files.empty()) {
        return refusal(cx, L"Checksum", L"Select one or more files to hash.");
    }
    std::wstring error;
    json::Value m;
    if (verify) {
        m = try_tool_call(cx.main, "checksum.verify", "{\"path\":" + json_quote(against) + "}", &error);
    } else {
        std::vector<std::wstring> paths;
        for (const SelectedRow& r : files) paths.push_back(r.path);
        m = try_tool_call(cx.main, "checksum.hash", "{\"paths\":" + json_array(paths) + "}", &error);
    }
    if (m.is_null()) return refusal(cx, L"Checksum", error);
    return checksum_card(cx, m);
}

std::unique_ptr<Panel> checksum_show(DialogContext& cx) {
    std::wstring error;
    const json::Value m = try_tool_call(cx.main, "checksum.model", {}, &error);
    if (m.is_null()) return refusal(cx, L"Checksum", error);
    return checksum_card(cx, m);
}

// ---- comparing two files ---------------------------------------------------------------------------------

std::unique_ptr<Panel> compare_files_card(DialogContext& cx, const json::Value& m) {
    Card c = make_card(L"Compare files", 1000);
    auto* names = f::row(*c.body, 10);
    auto* l = f::text(*names, W(m["leftName"]), tok::TextPrimary, 14.0f, 600);
    l->name = "CompareLeftName";
    l->tooltip = W(m["leftPath"]);
    l->vcenter = true;
    l->flex = 1;
    auto* swap = f::button(*names, L"Swap", [cx] {
        try_tool_call(cx.main, "filecompare.swap");
        cx.main.defer([main = &cx.main] { main->open_dialog(L"compare-files-show"); });
    }, 70);
    swap->name = "CompareSwap";
    swap->tooltip = L"Put each file on the other side.";
    auto* r = f::text(*names, W(m["rightName"]), tok::TextPrimary, 14.0f, 600);
    r->name = "CompareRightName";
    r->tooltip = W(m["rightPath"]);
    r->vcenter = true;
    r->flex = 1;
    // Said while the files are being read, over whatever the last comparison showed. Spec: FileCompareWindow.xaml.
    const std::wstring said = m["comparing"].boolean() ? std::wstring(L"Comparing…") : W(m["verdict"]);
    auto* verdict = f::text(*c.body, said, m["identical"].boolean() ? tok::TextLink : tok::TextSecondary, 13.5f);
    verdict->name = "CompareFilesVerdict";
    if (!m["banner"].is_null()) f::text(*c.body, W(m["banner"]), tok::WarningForeground, 12.5f)->wrap = true;

    auto* diff = c.body->emplace<tw::DiffView>();
    diff->name = "DiffView";
    diff->height = 380;
    std::vector<tw::DiffLine> lines;
    for (size_t i = 0; i < m["rows"].size(); ++i) {
        const json::Value& row = m["rows"][i];
        tw::DiffLine d;
        const std::string op = row["op"].str();
        d.op = op == "delete" ? 1 : op == "insert" ? 2 : op == "replace" ? 3 : 0;
        d.ln = W(row["ln"]);
        d.lt = W(row["lt"]);
        d.rn = W(row["rn"]);
        d.rt = W(row["rt"]);
        lines.push_back(std::move(d));
    }
    diff->set_lines(std::move(lines));
    if (!m["scrollTo"].is_null()) diff->scroll_to_row(static_cast<int>(m["scrollTo"].num()));

    auto* foot = f::row(*c.body, 12);
    auto* counter = f::text(*foot, W(m["counter"]), tok::TextSecondary, 13.0f);
    counter->width = 170;
    const bool can_step = m["canStep"].boolean();
    auto step = [cx](int dir) {
        try_tool_call(cx.main, "filecompare.step", "{\"direction\":" + std::to_string(dir) + "}");
        cx.main.open_dialog(L"compare-files-show");
    };
    f::button(*foot, L"Previous", [step] { step(-1); }, 96)->set_enabled(can_step);
    f::button(*foot, L"Next", [step] { step(1); }, 96)->set_enabled(can_step);
    const bool text_mode = m["mode"].str() == "text";
    const bool ws = m["ignoreWhitespace"].boolean(), cs = m["ignoreCase"].boolean();
    auto apply = [cx](bool whitespace, bool ignore_case, bool force) {
        tools_ui(cx.main).compare_force_text = force;
        try_tool_call(cx.main, "filecompare.options",
                      std::string("{\"ignoreWhitespace\":") + (whitespace ? "true" : "false") + ",\"ignoreCase\":" + (ignore_case ? "true" : "false") +
                          ",\"forceText\":" + (force ? "true" : "false") + "}");
        cx.main.open_dialog(L"compare-files-show");
    };
    const bool force = tools_ui(cx.main).compare_force_text;
    if (text_mode) {
        f::check(*foot, L"Ignore trailing whitespace", ws, [apply, cs, force](bool on) { apply(on, cs, force); });
        f::check(*foot, L"Ignore case", cs, [apply, ws, force](bool on) { apply(ws, on, force); });
    }
    if (m["canForceText"].boolean()) {
        f::button(*foot, L"Compare as text", [apply, ws, cs] { apply(ws, cs, true); }, 150)->name = "ForceText";
    }
    add_button(c, L"Close", cx.close, true);
    return std::move(c.root);
}

std::unique_ptr<Panel> compare_files_start(DialogContext& cx) {
    tools_ui(cx.main).compare_force_text = false;
    const std::vector<SelectedRow> files = selected_files(cx.main);
    if (files.size() != 2) return refusal(cx, L"Compare files", L"Select exactly two files to compare.");
    std::wstring error;
    const json::Value m = try_tool_call(cx.main, "filecompare.open",
                                        "{\"left\":" + json_quote(files[0].path) + ",\"right\":" + json_quote(files[1].path) + "}", &error);
    if (m.is_null()) return refusal(cx, L"Compare files", error);
    return compare_files_card(cx, m);
}

std::unique_ptr<Panel> compare_files_show(DialogContext& cx) {
    std::wstring error;
    const json::Value m = try_tool_call(cx.main, "filecompare.model", {}, &error);
    if (m.is_null()) return refusal(cx, L"Compare files", error);
    return compare_files_card(cx, m);
}

// ---- disk usage ------------------------------------------------------------------------------------------

std::vector<tw::TreemapCell> treemap_cells(MainView& main, const std::vector<double>& weights, float w, float h) {
    std::string args = "{\"weights\":[";
    for (size_t i = 0; i < weights.size(); ++i) args += (i ? "," : "") + std::to_string(weights[i]);
    args += "],\"width\":" + std::to_string(w) + ",\"height\":" + std::to_string(h) + "}";
    const json::Value v = try_tool_call(main, "treemap.layout", args);
    std::vector<tw::TreemapCell> out;
    for (size_t i = 0; i < v.size(); ++i) {
        out.push_back({static_cast<size_t>(v[i]["index"].num()), static_cast<float>(v[i]["x"].num()), static_cast<float>(v[i]["y"].num()),
                       static_cast<float>(v[i]["width"].num()), static_cast<float>(v[i]["height"].num())});
    }
    return out;
}

std::unique_ptr<Panel> disk_usage_card(DialogContext& cx, const json::Value& m, bool loading) {
    Card c = make_card(L"Disk usage", 900);
    // Everything that can come and go on a load sits in a slot that is always there, so drilling into a
    // folder never moves the rest of the window: the progress strip, the footnote and the banner.
    c.body->height = 470;
    const std::wstring root = W(m["root"]);
    auto open_at = [cx](const std::wstring& path) {
        tools_ui(cx.main).disk_root = path;
        cx.main.open_dialog(L"disk-usage-at");
    };

    auto* head = f::row(*c.body, 8);
    head->height = 30;
    auto* up = f::button(*head, L"Up", [open_at, root] { open_at(parent_of(root)); }, 56);
    up->name = "DiskUp";
    up->set_enabled(m["canGoUp"].boolean());
    auto* where = f::text(*head, W(m["rootDisplay"]), tok::TextPrimary, 14.0f);
    where->name = "DiskUsageRoot";
    where->flex = 1;
    where->vcenter = true;
    f::button(*head, L"Refresh", [open_at, root] { open_at(root); }, 76);

    // Indeterminate while a folder is being read; the slot stays when it is not.
    auto* progress = c.body->emplace<Panel>();
    progress->height = 3;
    if (loading) {
        auto* bar = progress->emplace<ProgressBar>();
        bar->name = "DiskUsageLoading";
        bar->set_indeterminate(true);
    }

    auto* cols = f::row(*c.body, 12);
    cols->flex = 1;

    // ---- the folder's composition: map, then list ----
    auto* left = f::column(*cols, 6);
    left->flex = 1;
    auto* title = f::row(*left, 8);
    title->height = 22;
    auto* biggest = f::text(*title, L"Biggest items here", tok::TextPrimary, 14.0f, 600);
    biggest->flex = 1;
    f::text(*title, W(m["totalText"]), tok::TextSecondary, 13.0f)->name = "DiskUsageTotal";

    const json::Value& children = m["children"];
    auto* map = left->emplace<tw::TreemapCanvas>();
    map->name = "Treemap";
    map->height = 180;
    map->style.bg = tok::InputBackground;
    for (size_t i = 0; i < m["fills"].size(); ++i) map->fills.push_back(static_cast<uint32_t>(m["fills"][i].num()));
    MainView* main = &cx.main;
    map->layouter = [main](const std::vector<double>& w, float width, float height) { return treemap_cells(*main, w, width, height); };
    // Only measured children get area: an unknown size has no share to draw. Past the cap the rest are
    // folded into one tile that is not a folder.
    std::vector<tw::TreemapTile> tiles;
    std::vector<std::wstring> tile_paths;
    std::vector<bool> tile_dirs;
    const size_t max_tiles = static_cast<size_t>(m["maxTiles"].num(300));
    double folded_weight = 0;
    size_t folded = 0;
    for (size_t i = 0; i < children.size(); ++i) {
        const json::Value& ch = children[i];
        if (ch["unknown"].boolean() || ch["size"].num() <= 0) continue;
        if (tiles.size() >= max_tiles) {
            folded_weight += ch["size"].num();
            ++folded;
            continue;
        }
        tiles.push_back({W(ch["name"]), W(ch["sizeText"]), ch["size"].num(), false});
        tile_paths.push_back(ch["synthetic"].boolean() ? std::wstring() : W(ch["path"]));
        tile_dirs.push_back(ch["isDirectory"].boolean());
    }
    if (folded > 0) {
        tiles.push_back({std::to_wstring(folded) + L" smaller items", L"", folded_weight, true});
        tile_paths.push_back({});
        tile_dirs.push_back(false);
    }
    MainView* main_view = &cx.main;
    auto reveal = [main_view](const std::wstring& path, bool dir) {
        try_tool_call(*main_view, "reveal", "{\"path\":" + json_quote(path) + ",\"isDirectory\":" + (dir ? "true" : "false") + "}");
    };
    map->on_tile = [open_at, reveal, tile_paths, tile_dirs](size_t i) {
        if (i >= tile_paths.size() || tile_paths[i].empty()) return;
        if (tile_dirs[i]) open_at(tile_paths[i]);
        else reveal(tile_paths[i], false);
    };
    map->set_tiles(std::move(tiles));

    auto* list = left->emplace<tw::TableList>();
    list->name = "DiskUsageChildren";
    list->flex = 1;
    list->show_header = false;
    list->columns = {{L"Name", 0, true, false, false}, {L"Size", 110, false, true, false}};
    list->picture_column = 0;
    std::vector<tw::TableRow> rows;
    std::vector<std::wstring> row_paths;
    std::vector<bool> row_dirs;
    for (size_t i = 0; i < children.size(); ++i) {
        const json::Value& ch = children[i];
        tw::TableRow row;
        row.cells = {W(ch["name"]), W(ch["sizeText"])};
        if (!ch["synthetic"].boolean()) row.picture = file_picture(cx.main, W(ch["path"]), ch["isDirectory"].boolean());
        // The bar is the measurement made visible, so an unmeasured row gets none: an empty bar is the
        // honest picture of a share nobody has established.
        row.bar = ch["unknown"].boolean() ? -1.0f : static_cast<float>(ch["fraction"].num());
        row.bar_slot = true;
        row.muted = ch["unknown"].boolean();
        rows.push_back(std::move(row));
        row_paths.push_back(ch["synthetic"].boolean() ? std::wstring() : W(ch["path"]));
        row_dirs.push_back(ch["isDirectory"].boolean());
    }
    list->set_rows(std::move(rows));
    list->on_activate = [open_at, reveal, row_paths, row_dirs](int i) {
        if (i < 0 || static_cast<size_t>(i) >= row_paths.size() || row_paths[static_cast<size_t>(i)].empty()) return;
        if (row_dirs[static_cast<size_t>(i)]) open_at(row_paths[static_cast<size_t>(i)]);
        else reveal(row_paths[static_cast<size_t>(i)], false);
    };
    auto* foot = left->emplace<Panel>();
    foot->height = 15;
    if (m["unknownChildCount"].num() > 0) {
        f::text(*foot, std::to_wstring(static_cast<int>(m["unknownChildCount"].num())) + L" folders have never been measured and are shown without a size.", tok::TextSecondary, 11.0f)
            ->name = "DiskUsageFootnote";
    }

    // ---- the biggest files under here ----
    auto* right = f::column(*cols, 6);
    right->flex = 1;
    auto* rtitle = f::text(*right, L"Largest files below here", tok::TextPrimary, 14.0f, 600);
    rtitle->height = 22;
    auto* files = right->emplace<tw::TableList>();
    files->name = "DiskUsageLargestFiles";
    files->flex = 1;
    files->show_header = false;
    files->columns = {{L"Name", 0, true, false, false}, {L"Folder", 0, true, false, false}, {L"Size", 84, false, true, false}};
    files->picture_column = 0;
    std::vector<tw::TableRow> frows;
    std::vector<std::wstring> fpaths;
    for (size_t i = 0; i < m["largest"].size(); ++i) {
        const json::Value& fl = m["largest"][i];
        tw::TableRow row;
        row.cells = {W(fl["name"]), W(fl["relativePath"]), W(fl["sizeText"])};
        row.picture = file_picture(cx.main, W(fl["path"]), false);
        frows.push_back(std::move(row));
        fpaths.push_back(W(fl["path"]));
    }
    files->empty_text = W(m["largestEmpty"]);
    files->set_rows(std::move(frows));
    files->on_activate = [reveal, fpaths](int i) {
        if (i >= 0 && static_cast<size_t>(i) < fpaths.size()) reveal(fpaths[static_cast<size_t>(i)], false);
    };

    // The banner explains an answer that cannot be drawn as numbers: an unindexed drive must not read
    // like an empty one. Docked at the bottom in a slot of its own.
    auto* banner_slot = c.body->emplace<Panel>();
    banner_slot->height = 40;
    if (m["hasMessage"].boolean()) {
        auto* banner = banner_slot->emplace<StackPanel>(Orientation::Horizontal, 10.0f);
        banner->name = "DiskUsageBanner";
        banner->style.bg = tok::WarningBackground;
        banner->pad = 8;
        auto* text = f::text(*banner, W(m["message"]), tok::WarningForeground, 13.0f);
        text->flex = 1;
        text->wrap = true;
        if (m["canRetry"].boolean()) {
            // What the banner offers is a start (attach, or launch the helper); a retry is only for a failed build.
            f::button(*banner, L"Try again", [cx] { bb_index_do(cx.main.core().app(), core::s(L"start")); }, 90);
        }
    }
    add_button(c, L"Close", cx.close, true);
    return std::move(c.root);
}

// The window from where the read stands: the folder being read says so until its answer arrives (the last
// answer stays on show meanwhile, under a moving bar).
std::unique_ptr<Panel> disk_usage_from(DialogContext& cx, const json::Value& st) {
    const bool loading = st["loading"].boolean();
    if (!st["model"].is_null()) return disk_usage_card(cx, st["model"], loading);
    if (!loading) return refusal(cx, L"Disk usage", st["error"].is_null() ? L"Nothing to show." : W(st["error"]));
    Card c = make_card(L"Disk usage", 900);
    c.body->height = 470;
    f::text(*c.body, W(st["path"]), tok::TextPrimary, 14.0f);
    auto* bar = c.body->emplace<ProgressBar>();
    bar->name = "DiskUsageLoading";
    bar->height = 3;
    bar->set_indeterminate(true);
    f::note(*c.body, L"Reading the folder\u2026");
    add_button(c, L"Close", cx.close, true);
    return std::move(c.root);
}

std::unique_ptr<Panel> disk_usage_for(DialogContext& cx, const std::wstring& root) {
    std::wstring error;
    const json::Value st = try_tool_call(cx.main, "diskusage.load", "{\"path\":" + json_quote(root) + "}", &error);
    if (st.is_null()) return refusal(cx, L"Disk usage", error);
    return disk_usage_from(cx, st);
}

// ---- the change timeline ---------------------------------------------------------------------------------

Token change_colour(const std::string& kind) {
    if (kind == "Created") return tok::ChangeCreated;
    if (kind == "Deleted") return tok::ChangeDeleted;
    if (kind == "Renamed") return tok::ChangeRenamed;
    return tok::ChangeModified;
}

std::unique_ptr<Panel> changes_card(DialogContext& cx, const json::Value& m) {
    Card c = make_card(L"What changed", 940);
    auto set = [cx](const std::string& args) {
        try_tool_call(cx.main, "changes.set", args);
        cx.main.open_dialog(L"changes-show");
    };
    auto* bar = f::row(*c.body, 12);
    std::vector<std::wstring> labels;
    int current = 0;
    std::vector<std::string> ids;
    for (size_t i = 0; i < m["ranges"].size(); ++i) {
        labels.push_back(W(m["ranges"][i]["label"]));
        ids.push_back(m["ranges"][i]["id"].str());
        if (m["ranges"][i]["id"].str() == m["range"].str()) current = static_cast<int>(i);
    }
    auto* range = f::combo(*bar, labels, current, 170, [set, ids](int i) {
        if (i >= 0 && static_cast<size_t>(i) < ids.size()) set("{\"range\":\"" + ids[static_cast<size_t>(i)] + "\"}");
    });
    range->name = "ChangesRange";
    f::check(*bar, L"Created", m["created"].boolean(), [set](bool on) { set(std::string("{\"created\":") + (on ? "true" : "false") + "}"); });
    f::check(*bar, L"Modified", m["modified"].boolean(), [set](bool on) { set(std::string("{\"modified\":") + (on ? "true" : "false") + "}"); });
    f::check(*bar, L"Deleted", m["deleted"].boolean(), [set](bool on) { set(std::string("{\"deleted\":") + (on ? "true" : "false") + "}"); });
    f::check(*bar, L"Renamed", m["renamed"].boolean(), [set](bool on) { set(std::string("{\"renamed\":") + (on ? "true" : "false") + "}"); });
    f::check(*bar, L"Hidden", m["hidden"].boolean(), [set](bool on) { set(std::string("{\"hidden\":") + (on ? "true" : "false") + "}"); });
    auto* spacer = bar->emplace<Widget>();
    spacer->flex = 1;
    f::button(*bar, L"Mark now", [cx] {
        try_tool_call(cx.main, "changes.mark");
        cx.main.open_dialog(L"changes-show");
    }, 96)->name = "ChangesMark";
    f::button(*bar, L"Refresh", [cx] { cx.main.open_dialog(L"changes-show"); }, 84);

    auto* scope = f::row(*c.body, 12);
    f::text(*scope, L"Watching  " + W(m["scopeDisplay"]), tok::TextSecondary, 13.0f)->name = "ChangesScope";
    if (!m["markText"].str().empty()) f::text(*scope, W(m["markText"]), tok::TextMuted, 13.0f);

    // A switched-off feature must not look like a quiet disk, so why there is nothing to show is said
    // before the list.
    if (m["hasMessage"].boolean()) {
        auto* banner = f::row(*c.body, 10);
        banner->name = "ChangesBanner";
        banner->style.bg = tok::WarningBackground;
        banner->pad = 8;
        auto* text = f::text(*banner, W(m["message"]), tok::WarningForeground, 13.0f);
        text->flex = 1;
        text->wrap = true;
        if (m["canOpenSettings"].boolean()) {
            f::button(*banner, L"Settings", [cx] {
                cx.close();
                bb_settings_page_open(cx.main.core().app(), core::s(L"history"));
                cx.main.sync_overlays();
            }, 90)->name = "ChangesOpenSettings";
        }
        if (m["canRetry"].boolean()) {
            // What the banner offers is a start (attach, or launch the helper); a retry is only for a failed build.
            f::button(*banner, L"Try again", [cx] { bb_index_do(cx.main.core().app(), core::s(L"start")); }, 90);
        }
    }

    auto* table = c.body->emplace<tw::TableList>();
    table->name = "ChangesList";
    table->height = 330;
    table->columns = {{L"Change", 90, false, false, false}, {L"Name", 0, true, false, false}, {L"Folder", 0, true, false, false}, {L"When", 110, false, true, false}};
    std::vector<tw::TableRow> rows;
    for (size_t i = 0; i < m["rows"].size(); ++i) {
        const json::Value& r = m["rows"][i];
        tw::TableRow row;
        std::wstring name = W(r["name"]);
        if (!r["renamedFrom"].str().empty()) name += L"   (" + W(r["renamedFrom"]) + L")";
        if (!r["countText"].str().empty()) name += L"   " + W(r["countText"]);
        row.cells = {W(r["kind"]), name, W(r["folder"]), W(r["when"])};
        // First and last time, and how often, for a row that folds several. Spec: ChangeEventViewModel.WhenToolTip.
        const std::wstring last = format_file_time(r["lastUtc"].i64());
        row.tip = r["count"].i64() > 1 ? L"First " + format_file_time(r["firstUtc"].i64()) + L", last " + last + L" \u2014 " +
                                             W(r["countText"]).substr(1) + L" times"
                                       : last;
        row.tint = change_colour(r["kind"].str());
        row.muted = r["deleted"].boolean();
        rows.push_back(std::move(row));
    }
    table->empty_text = W(m["emptyMessage"]);
    table->set_rows(std::move(rows));
    // A double-click shows the item where it is, or the folder it was in once it is gone.
    // Spec: ChangeTimelineWindow.Row_DoubleClick.
    const json::Value list = m["rows"];
    MainView* main_view = &cx.main;
    table->on_activate = [main_view, list](int i) {
        if (i < 0 || static_cast<size_t>(i) >= list.size()) return;
        const json::Value& r = list[static_cast<size_t>(i)];
        const std::wstring path = W(r["path"]), folder = W(r["folder"]);
        const DWORD attrs = GetFileAttributesW(path.c_str());
        const bool there = attrs != INVALID_FILE_ATTRIBUTES && !r["deleted"].boolean();
        const std::wstring target = there ? path : folder;
        const bool dir = there ? (attrs & FILE_ATTRIBUTE_DIRECTORY) != 0 : true;
        if (target.empty() || (!there && GetFileAttributesW(folder.c_str()) == INVALID_FILE_ATTRIBUTES)) return;
        try_tool_call(*main_view, "reveal", "{\"path\":" + json_quote(target) + ",\"isDirectory\":" + (dir ? "true" : "false") + "}");
    };
    f::text(*c.body, W(m["summary"]), tok::TextSecondary, 13.0f)->name = "ChangesSummary";
    // The picked rows as text, one tab-separated line each, so they paste into a spreadsheet as columns:
    // what changed, the path, when (local), how often, and for a rename where from.
    // Spec: ChangeTimelineWindow.Copy, ChangeEventViewModel.CopyLine.
    Widget* anchor = c.root.get();
    table->multi = true;
    auto copy = [main_view, table, list, anchor] {
        std::wstring text;
        int n = 0;
        for (int i : table->picked()) {
            if (static_cast<size_t>(i) >= list.size()) continue;
            const json::Value& r = list[static_cast<size_t>(i)];
            std::wstring line = W(r["kind"]) + L"\t" + W(r["path"]) + L"\t" + format_file_time(r["lastUtc"].i64());
            if (r["count"].i64() > 1) line += L"\t" + W(r["countText"]);
            if (!r["oldPath"].is_null() && r["kind"].str() == "Renamed") line += L"\tfrom " + W(r["oldPath"]);
            text += (n++ ? L"\r\n" : L"") + line;
        }
        if (text.empty()) return;
        if (anchor->root()) anchor->root()->clipboard.write(text);
        bb_status_set(main_view->core().app(), core::s(n == 1 ? std::wstring(L"Copied the change") : L"Copied " + std::to_wstring(n) + L" changes"));
    };
    table->on_copy = copy;
    add_button(c, L"Copy", copy)->name = "ChangesCopy";
    add_button(c, L"Close", cx.close, true);
    return std::move(c.root);
}

std::unique_ptr<Panel> changes_start(DialogContext& cx) {
    // From the tree's menu, the folder right-clicked; otherwise the folder the tab shows.
    const std::wstring scope = cx.main.dialog_folder().empty() ? cx.main.core().tab_text(cx.main.active_tab(), BB_TEXT_PATH)
                                                               : cx.main.dialog_folder();
    std::wstring error;
    const json::Value m = try_tool_call(cx.main, "changes.open", "{\"path\":" + json_quote(scope) + "}", &error);
    if (m.is_null()) return refusal(cx, L"What changed", error);
    return changes_card(cx, m);
}

std::unique_ptr<Panel> changes_show(DialogContext& cx) {
    std::wstring error;
    const json::Value m = try_tool_call(cx.main, "changes.model", {}, &error);
    if (m.is_null()) return refusal(cx, L"What changed", error);
    return changes_card(cx, m);
}

// ---- the duplicate finder --------------------------------------------------------------------------------

// "What do I have two of?": files that are byte-for-byte identical, grouped, with the copies ticked here handed
// to the ordinary delete confirmation. The scan is the core's, off the UI thread, and only ever started by Scan:
// it reads real files. Ticks are this window's, by position, and go whenever the groups are replaced. One copy
// of every group always stays. Spec: Views/DuplicatesWindow, ViewModels/DuplicatesViewModel.
std::unique_ptr<Panel> duplicates_card(DialogContext& cx) {
    Card c = make_card(L"Duplicate files", 860);
    MainView& main = cx.main;
    DuplicatesState& d = main.duplicates();
    if (!d.has) main.sync_duplicates();
    const json::Value& m = d.result;
    const bool scanning = m["scanning"].boolean();

    // Where, and Scan / Stop.
    auto* top = f::row(*c.body, 10);
    auto* scan = f::button(*top, scanning ? L"Stop" : L"Scan", [cx, scanning] {
        if (scanning) {
            tool_call(cx.main, "duplicates.cancel");
            return;
        }
        const json::Value& now = cx.main.duplicates().result;
        const std::string root = now["root"].is_null() ? "null" : json_quote(W(now["root"]));
        tool_call(cx.main, "duplicates.scan", "{\"root\":" + root + "}");
    }, 90);
    scan->name = scanning ? "DuplicatesStop" : "DuplicatesScan";
    scan->primary = !scanning;
    auto* where = f::text(*top, W(m["rootDisplay"]), tok::TextPrimary, 13.0f);
    where->name = "DuplicatesRoot";
    where->flex = 1;

    // The knobs, remembered for next time.
    auto* knobs = f::row(*c.body, 10);
    f::text(*knobs, L"Ignore files under", tok::TextSecondary, 13.0f);
    std::vector<std::wstring> labels;
    std::vector<int64_t> sizes;
    int at = 0;
    for (size_t i = 0; i < m["minSizes"].size(); ++i) {
        labels.push_back(W(m["minSizes"][i]["label"]));
        sizes.push_back(m["minSizes"][i]["bytes"].i64());
        if (sizes.back() == m["minSize"].i64()) at = static_cast<int>(i);
    }
    auto* floor = f::combo(*knobs, labels, at, 110, [cx, sizes](int i) {
        if (i < 0 || static_cast<size_t>(i) >= sizes.size()) return;
        bb_settings_set(cx.main.core().app(), core::s(L"duplicateMinSizeBytes"), core::s(std::to_wstring(sizes[static_cast<size_t>(i)])));
        cx.main.sync_duplicates();
    });
    floor->name = "DuplicatesMinSize";
    floor->set_enabled(!scanning);
    auto* skip = f::check(*knobs, L"Skip Windows and Program Files", m["skipSystem"].boolean(), [cx](bool on) {
        bb_settings_set(cx.main.core().app(), core::s(L"duplicateSkipSystemFolders"), core::s(on ? L"true" : L"false"));
        cx.main.sync_duplicates();
    });
    skip->name = "DuplicatesSkipSystem";
    skip->set_enabled(!scanning);

    // Why the answer cannot be believed, when it cannot: an unindexed drive must not look like a tidy one.
    if (!m["message"].str().empty()) {
        auto* banner = f::row(*c.body, 10);
        banner->name = "DuplicatesBanner";
        banner->style.bg = tok::WarningBackground;
        banner->pad = 8;
        auto* text = f::text(*banner, W(m["message"]), tok::WarningForeground, 13.0f);
        text->flex = 1;
        text->wrap = true;
        if (m["canRetry"].boolean()) {
            f::button(*banner, L"Try again", [cx] { bb_index_do(cx.main.core().app(), core::s(L"start")); }, 90);
        }
    }

    if (scanning) {
        const json::Value& p = m["progress"];
        auto* line = f::text(*c.body, W(p["headline"]) + (p["detail"].str().empty() ? L"" : L"  " + W(p["detail"])), tok::TextSecondary, 13.0f);
        line->name = "DuplicatesProgress";
        auto* bar = c.body->emplace<ProgressBar>();
        bar->height = 6;
        bar->set_indeterminate(p["indeterminate"].boolean());
        if (!p["indeterminate"].boolean()) bar->set_value(p["fraction"].num());
    }

    const json::Value& groups = m["groups"];
    // The summary counts what is ticked here, so it is said here.
    size_t ticked = 0;
    int64_t ticked_bytes = 0;
    for (const auto& [g, i] : d.ticked) {
        if (g < groups.size() && i < groups[g]["files"].size()) {
            ++ticked;
            ticked_bytes += groups[g]["sizeBytes"].i64();
        }
    }
    auto* picks = f::row(*c.body, 8);
    if (groups.size() > 0) {
        std::wstring summary = std::to_wstring(groups.size()) + L" groups \u00b7 " + format_bytes(m["wastedBytes"].i64()) + L" reclaimable";
        if (ticked > 0) summary += L" \u00b7 " + std::to_wstring(ticked) + L" selected (" + format_bytes(ticked_bytes) + L")";
        auto* sum = f::text(*picks, summary, tok::TextSecondary, 13.0f);
        sum->name = "DuplicatesSummary";
        sum->flex = 1;
        f::button(*picks, L"None", [cx] {
            cx.main.duplicates().ticked.clear();
            cx.main.open_dialog(L"duplicates");
        }, 70)->name = "Keep-none";
        for (const char* strategy : {"oldest", "newest", "shallowest"}) {
            const std::string st = strategy;
            std::wstring label = L"Keep " + core::wide(st);
            f::button(*picks, label, [cx, st] {
                DuplicatesState& ds = cx.main.duplicates();
                ds.ticked.clear();
                const json::Value& gs = ds.result["groups"];
                for (size_t g = 0; g < gs.size(); ++g) {
                    const size_t kept = static_cast<size_t>(gs[g]["keep"][st].i64());
                    for (size_t i = 0; i < gs[g]["files"].size(); ++i) {
                        if (i != kept) ds.ticked.insert({g, i});
                    }
                }
                cx.main.open_dialog(L"duplicates");
            }, 130)->name = "Keep-" + st;
        }
    }

    auto* table = c.body->emplace<tw::TableList>();
    table->name = "DuplicateRows";
    table->height = 330;
    table->empty_text = W(m["emptyMessage"]);
    table->columns = {{L"", 34, false, false, false},
                      {L"Name", 0, true, false, false},
                      {L"Folder", 0, true, false, false},
                      {L"Modified", 140, false, false, false},
                      {L"Size", 90, false, true, false}};
    table->picture_column = 1;
    std::vector<tw::TableRow> rows;
    std::vector<std::pair<size_t, size_t>> index;  // per row: group, file (file == npos for a heading)
    for (size_t g = 0; g < groups.size(); ++g) {
        tw::TableRow h;
        h.heading = true;
        const int64_t size = groups[g]["sizeBytes"].i64();
        const size_t n = groups[g]["files"].size();
        h.cells = {L"", std::to_wstring(n) + L" copies of " + format_bytes(size), L"",
                   format_bytes(size * static_cast<int64_t>(n > 0 ? n - 1 : 0)) + L" spare", L""};
        rows.push_back(std::move(h));
        index.push_back({g, static_cast<size_t>(-1)});
        for (size_t i = 0; i < n; ++i) {
            const json::Value& file = groups[g]["files"][i];
            const bool on = d.ticked.count({g, i}) > 0;
            tw::TableRow row;
            std::wstring folder = file["dir"].str().empty() ? std::wstring(L"(this folder)") : W(file["dir"]);
            // Spec: DuplicateGroupViewModel.HardlinkNote.
            const size_t links = file["hardlinks"].size();
            if (links == 1) folder += L"  ·  Also linked as " + W(file["hardlinks"][0]);
            else if (links > 1) folder += L"  ·  Also linked under " + std::to_wstring(links) + L" other names";
            row.cells = {on ? L"\u2611" : L"\u2610", W(file["name"]), folder, format_file_time(file["modified"].i64()), format_bytes(file["sizeBytes"].i64())};
            row.picture = file_picture(cx.main, W(file["path"]), false);
            row.faded = file["hidden"].boolean();
            // The last copy a group has left cannot be ticked; the row says so rather than the box springing
            // back. Spec: DuplicateGroupViewModel.CanTick.
            if (!on) {
                size_t ticked_here = 0;
                for (const auto& t : d.ticked) ticked_here += t.first == g ? 1 : 0;
                if (ticked_here + 1 >= n) {
                    row.cells[0] = L"‒";
                    row.tip = L"One copy always stays — untick another first.";
                }
            }
            row.muted = !on;
            rows.push_back(std::move(row));
            index.push_back({g, i});
        }
    }
    table->set_rows(std::move(rows));
    // A click on the box (or Space) ticks; a double-click shows the copy where it lives, which is how two
    // identical files are told apart before deciding which to keep.
    table->on_toggle = [cx, index](int i) {
        if (i < 0 || static_cast<size_t>(i) >= index.size() || index[static_cast<size_t>(i)].second == static_cast<size_t>(-1)) return;
        DuplicatesState& st = cx.main.duplicates();
        const auto key = index[static_cast<size_t>(i)];
        if (!st.ticked.erase(key)) {
            // One copy always stays: the last unticked one cannot be ticked. Spec: DuplicateGroupViewModel.CanTick.
            size_t ticked_here = 0;
            for (const auto& t : st.ticked) ticked_here += t.first == key.first ? 1 : 0;
            if (ticked_here + 1 >= st.result["groups"][key.first]["files"].size()) {
                bb_status_set(cx.main.core().app(), core::s(L"One copy always stays - untick another first."));
                return;
            }
            st.ticked.insert(key);
        }
        cx.main.open_dialog(L"duplicates");
    };
    table->on_activate = [cx, index](int i) {
        if (i < 0 || static_cast<size_t>(i) >= index.size() || index[static_cast<size_t>(i)].second == static_cast<size_t>(-1)) return;
        const auto [g, f] = index[static_cast<size_t>(i)];
        const json::Value& file = cx.main.duplicates().result["groups"][g]["files"][f];
        tool_call(cx.main, "reveal", "{\"path\":" + json_quote(W(file["path"])) + ",\"isDirectory\":false}");
    };

    if (!m["caveat"].str().empty()) f::text(*c.body, W(m["caveat"]), tok::TextSecondary, 13.0f)->name = "DuplicatesCaveat";

    auto* remove = add_button(c, L"Delete selected copies\u2026", [cx] { duplicates_ask_remove(cx.main); }, false, 190);
    remove->name = "RemoveTicked";
    remove->set_enabled(ticked > 0 && !scanning);
    add_button(c, L"Close", cx.close, true);
    return std::move(c.root);
}

// ---- the folder comparison's sync ------------------------------------------------------------------------

struct SyncView {
    json::Value preview;
    std::vector<std::string> ticked;  // keys that will run
};

SyncView sync_view(MainView& main) {
    ToolsUi& ui = tools_ui(main);
    const std::string deletes = ui.sync_with_deletes ? "true" : "false";
    SyncView v;
    v.preview = try_tool_call(main, "compare.sync-preview", "{\"withDeletes\":" + deletes + "}");
    if (v.preview.is_null()) return v;
    for (size_t i = 0; i < v.preview["actions"].size(); ++i) {
        const json::Value& a = v.preview["actions"][i];
        if (a["ticked"].boolean() && !ui.sync_unticked.count(a["key"].str())) v.ticked.push_back(a["key"].str());
    }
    if (!ui.sync_unticked.empty()) {
        std::string keys = "[";
        for (size_t i = 0; i < v.ticked.size(); ++i) keys += (i ? "," : "") + json_quote(core::wide(v.ticked[i]));
        keys += "]";
        v.preview = try_tool_call(main, "compare.sync-preview", "{\"withDeletes\":" + deletes + ",\"ticked\":" + keys + "}");
    }
    return v;
}

std::unique_ptr<Panel> sync_card(DialogContext& cx, bool running) {
    std::wstring error;
    SyncView v = sync_view(cx.main);
    if (v.preview.is_null()) {
        const json::Value m = try_tool_call(cx.main, "compare.sync-preview", "{}", &error);
        return refusal(cx, L"Sync", error.empty() ? L"There is no comparison to sync. Compare two folders first." : error);
    }
    const json::Value& p = v.preview;
    ToolsUi& ui = tools_ui(cx.main);
    Card c = make_card(L"Sync", 780);
    // Named by the two folders' own names, the full paths beneath. Spec: SyncPreviewDialog (HeadingText).
    auto short_name = [](const std::wstring& path) {
        std::wstring p = path;
        while (p.size() > 3 && (p.back() == L'\\' || p.back() == L'/')) p.pop_back();
        const std::wstring name = bb_file_name(p);
        return name.empty() ? path : name;
    };
    f::text(*c.body, L"Make " + short_name(W(p["right"])) + L" match " + short_name(W(p["left"])), tok::TextPrimary, 15.0f)->wrap = true;
    f::text(*c.body, L"Left   " + W(p["left"]), tok::TextSecondary, 12.5f)->name = "SyncLeft";
    f::text(*c.body, L"Right  " + W(p["right"]), tok::TextSecondary, 12.5f)->name = "SyncRight";
    f::text(*c.body, W(p["summary"]), tok::TextSecondary, 13.0f)->name = "SyncSummary";

    auto* table = c.body->emplace<tw::TableList>();
    table->name = "SyncActions";
    table->height = 260;
    table->columns = {{L"", 34, false, false, false}, {L"Action", 80, false, false, false}, {L"Item", 0, true, false, false}, {L"Size", 90, false, true, false}, {L"Why", 170, false, false, false}};
    std::vector<tw::TableRow> rows;
    std::vector<std::string> keys;
    for (size_t i = 0; i < p["actions"].size(); ++i) {
        const json::Value& a = p["actions"][i];
        tw::TableRow row;
        row.cells = {a["ticked"].boolean() ? L"☑" : L"☐", W(a["kind"]), W(a["name"]), W(a["size"]), W(a["why"])};
        row.muted = !a["ticked"].boolean();
        row.fg = a["kind"].str() == "Delete" ? tok::ErrorForeground : gfx::kNoToken;
        rows.push_back(std::move(row));
        keys.push_back(a["key"].str());
    }
    table->empty_text = L"Nothing to sync.";
    table->set_rows(std::move(rows));
    if (!running) {
        // The box takes one click (or Space), as a checkbox does; a double-click on the row does too.
        table->on_toggle = [cx, keys](int i) {
            if (i < 0 || static_cast<size_t>(i) >= keys.size()) return;
            ToolsUi& st = tools_ui(cx.main);
            st.sync_touched = true;
            if (!st.sync_unticked.erase(keys[static_cast<size_t>(i)])) st.sync_unticked.insert(keys[static_cast<size_t>(i)]);
            cx.main.defer([main = &cx.main] { main->open_dialog(L"sync-preview-edit"); });
        };
        table->on_activate = table->on_toggle;
    }

    if (!p["caveat"].is_null()) f::text(*c.body, W(p["caveat"]), tok::WarningForeground, 13.0f)->wrap = true;
    auto* deletes = f::check(*c.body, L"Also delete items that are only on the right", ui.sync_with_deletes, [cx](bool on) {
        tools_ui(cx.main).sync_with_deletes = on;
        cx.main.open_dialog(L"sync-preview-edit");
    });
    deletes->name = "SyncWithDeletes";
    deletes->set_enabled(!running);
    f::note(*c.body, L"Undo puts the whole sync back, and they go to the Recycle Bin on any volume that has one.");

    const json::Value queue = try_model([&](BbOwnedStr* o) { return bb_ops_query(cx.main.core().app(), core::s(L"queue"), core::s(L""), o); });
    if (running && queue["active"].boolean()) {
        // The sync under way, live from the queue: what it is doing, how far through, and the way out. The
        // dialog goes once the run is done. Spec: SyncPreviewDialog.xaml (ProgressRow).
        MainView* main = &cx.main;
        auto progress = std::make_unique<ops::LiveBody>(cx.main, &MainView::on_sync_progress, [main, close = cx.close](ops::LiveBody& b) {
            const json::Value q = try_model([&](BbOwnedStr* o) { return bb_ops_query(main->core().app(), core::s(L"queue"), core::s(L""), o); });
            if (!q["active"].boolean()) {
                main->defer([close] { close(); });
                return;
            }
            const json::Value& v = q["view"];
            f::text(b, W(v["headline"]), tok::TextPrimary, 13.5f);
            auto* bar = b.emplace<ProgressBar>();
            bar->height = 6;
            bar->set_indeterminate(v["indeterminate"].boolean());
            if (!v["indeterminate"].boolean()) bar->set_value(v["fraction"].num());
            f::text(b, W(v["detail"]), tok::TextSecondary, 12.0f);
        });
        progress->name = "SyncProgress";
        c.body->add(std::move(progress));
        auto* cancel = add_button(c, L"Cancel", [main] { ops::run(main->core(), "cancel-running"); });
        cancel->tooltip = L"Stop the sync. What has already been copied stays, and undo still puts it back.";
        return std::move(c.root);
    }
    if (running) {
        // The run, reported where the list of what is being synced is; it replaces the buttons rather than
        // sitting beside them, since offering Sync again next to a running one invites pressing it. Posed on
        // a plan that is not moving, so the capture is deterministic.
        size_t copies = 0;
        for (size_t i = 0; i < p["actions"].size(); ++i) copies += p["actions"][i]["kind"].str() != "Delete" && p["actions"][i]["ticked"].boolean() ? 1 : 0;
        auto* run = f::column(*c.body, 4);
        run->name = "SyncProgress";
        f::text(*run, L"Copying 2 of " + std::to_wstring(std::max<size_t>(copies, 2)) + L" items", tok::TextPrimary, 13.5f);
        auto* bar = run->emplace<ProgressBar>();
        bar->set_value(0.34);
        bar->height = 6;
        f::text(*run, format_bytes(21500000) + L" of " + format_bytes(64LL * 1024 * 1024) + L"  ·  40 MB/s  ·  about 1 s left", tok::TextSecondary, 12.0f);
        add_button(c, L"Cancel", cx.close);
        return std::move(c.root);
    }

    auto* sync = add_button(c, L"Sync", [cx, ticked = v.ticked] {
        ToolsUi& st = tools_ui(cx.main);
        std::string list = "[";
        for (size_t i = 0; i < ticked.size(); ++i) list += (i ? "," : "") + json_quote(core::wide(ticked[i]));
        list += "]";
        try_tool_call(cx.main, "compare.sync", std::string("{\"withDeletes\":") + (st.sync_with_deletes ? "true" : "false") + ",\"ticked\":" + list + "}");
        // The dialog stays, showing the run, until it is done. Spec: SyncPreviewDialog (IsRunning).
        cx.main.defer([main = &cx.main] { main->open_dialog(L"sync-preview-live"); });
    }, true, 110);
    sync->name = "SyncRun";
    sync->set_enabled(p["canRun"].boolean());
    // Cancel is the default: a stray Enter backs out of a write that can delete. Spec: SyncPreviewDialog.xaml.
    add_button(c, L"Cancel", cx.close)->is_default = true;
    return std::move(c.root);
}

std::unique_ptr<Panel> sync_fresh(DialogContext& cx, bool running) {
    ToolsUi& ui = tools_ui(cx.main);
    ui.sync_with_deletes = false;
    ui.sync_unticked.clear();
    ui.sync_touched = false;
    return sync_card(cx, running);
}

// ---- the table ----------------------------------------------------------------------------------------------

using Builder = std::unique_ptr<Panel> (*)(DialogContext&);

const std::map<std::wstring, Builder>& builders() {
    static const std::map<std::wstring, Builder> table = {
        {L"checksum", [](DialogContext& cx) { return checksum_start(cx, false); }},
        {L"checksum-verify", [](DialogContext& cx) { return checksum_start(cx, true); }},
        {L"checksum-show", checksum_show},
        {L"compare-files", compare_files_start},
        {L"compare-files-show", compare_files_show},
        {L"disk-usage",
         [](DialogContext& cx) {
             tools_ui(cx.main).disk_root.clear();
             return disk_usage_for(cx, cx.main.core().tab_text(cx.main.active_tab(), BB_TEXT_PATH));
         }},
        {L"disk-usage-at", [](DialogContext& cx) { return disk_usage_for(cx, tools_ui(cx.main).disk_root); }},
        {L"disk-usage-show", [](DialogContext& cx) { return disk_usage_from(cx, try_tool_call(cx.main, "diskusage.state")); }},
        {L"changes", changes_start},
        {L"changes-show", changes_show},
        {L"duplicates", duplicates_card},
        {L"sync-preview", [](DialogContext& cx) { return sync_fresh(cx, false); }},
        {L"sync-preview-edit", [](DialogContext& cx) { return sync_card(cx, false); }},
        {L"sync-preview-running", [](DialogContext& cx) { return sync_fresh(cx, true); }},
        {L"sync-preview-live", [](DialogContext& cx) { return sync_card(cx, true); }},
    };
    return table;
}

}  // namespace

// ---- the duplicate finder's delete ------------------------------------------------------------------------

std::vector<std::wstring> duplicates_ticked_paths(MainView& main) {
    DuplicatesState& d = main.duplicates();
    std::vector<std::wstring> out;
    for (const auto& [g, i] : d.ticked) out.push_back(core::wide(d.result["groups"][g]["files"][i]["path"].str()));
    return out;
}

// Of the ticked copies, the ones a delete may take. The rule is the core's (`DuplicateRules::can_remove`):
// a group with every copy ticked contributes nothing rather than being emptied.
std::vector<std::wstring> duplicates_removable(MainView& main) {
    const std::vector<std::wstring> ticked = duplicates_ticked_paths(main);
    std::vector<std::wstring> out;
    if (ticked.empty()) return out;
    const json::Value r = tool_call(main, "duplicates.removable", "{\"paths\":" + json_array(ticked) + "}");
    for (size_t i = 0; i < r["paths"].size(); ++i) out.push_back(core::wide(r["paths"][i].str()));
    return out;
}

// What is still duplicated: the removed copies leave their groups, in the core (which the guard reads) and so
// here. The ticks go with them.
void duplicates_prune(MainView& main, const std::vector<std::wstring>& removed) {
    tool_call(main, "duplicates.prune", "{\"paths\":" + json_array(removed) + "}");
    main.duplicates().ticked.clear();
    main.sync_duplicates();
}

// The ticked copies, deleted at once - for a scripted run, which stands in for someone who said yes to the
// confirmation. The window's button asks first (`duplicates_ask_remove`).
bool duplicates_remove(MainView& main, uint64_t* op) {
    const std::vector<std::wstring> paths = duplicates_removable(main);
    if (paths.empty()) return false;
    std::vector<BbStr> views;
    for (const auto& p : paths) views.push_back(core::s(p));
    uint64_t id = 0;
    main.duplicates().pending_prune = paths;
    core::check(bb_op_delete(main.core().app(), views.data(), static_cast<uint32_t>(views.size()), BB_DELETE_RECYCLE, &id), "duplicates-remove");
    if (op) *op = id;
    return true;
}

void duplicates_settle_prune(MainView& main) {
    std::vector<std::wstring>& pending = main.duplicates().pending_prune;
    if (pending.empty()) return;
    std::vector<std::wstring> gone, still;
    for (const auto& p : pending) (GetFileAttributesW(p.c_str()) == INVALID_FILE_ATTRIBUTES ? gone : still).push_back(p);
    pending = std::move(still);
    if (!gone.empty()) duplicates_prune(main, gone);
}

// Remove ticked: the ordinary delete confirmation over the copies the rule allows, and back to this window
// afterwards with whatever was deleted gone from it. Spec: MainWindow.RemoveDuplicateCopies.
void duplicates_ask_remove(MainView& main) {
    const std::vector<std::wstring> paths = duplicates_removable(main);
    if (paths.empty()) {
        bb_status_set(main.core().app(), core::s(L"One copy of every file always stays - untick one in each group to remove the rest."));
        return;
    }
    // The confirmation is the main window's, and an owned window always stands above its owner: the
    // Duplicates window steps out of the way while it is asked, and comes back after.
    if (ToolWindow* tw = main.tool_window(L"duplicates"); tw && !tw->window().options().parked) ShowWindow(tw->window().hwnd(), SW_HIDE);
    main.ask_delete(paths, [&main, paths](bool confirmed) {
        // Pruned as the delete finishes, by what really went: a copy that could not be deleted stays listed.
        if (confirmed) main.duplicates().pending_prune = paths;
        main.defer([&main] {
            if (ToolWindow* tw = main.tool_window(L"duplicates"); tw && !tw->window().options().parked) ShowWindow(tw->window().hwnd(), SW_SHOW);
            main.open_dialog(L"duplicates");
        });
    });
}

// ---- the entry points ---------------------------------------------------------------------------------------

bool tool_dialog_exists(const std::wstring& kind) { return builders().count(kind) > 0; }

std::vector<std::wstring> tool_dialog_kinds() {
    std::vector<std::wstring> out;
    for (const auto& kv : builders()) out.push_back(kv.first);
    return out;
}

std::unique_ptr<Widget> tool_build_dialog(const std::wstring& kind, DialogContext ctx) {
    const auto it = builders().find(kind);
    if (it == builders().end()) return nullptr;
    return it->second(ctx);
}

}  // namespace bb::views
