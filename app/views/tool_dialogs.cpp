#include "views/tool_dialogs.h"

#include "views/forms.h"
#include "views/main_view.h"
#include "views/model.h"
#include "views/tool_state.h"
#include "views/tool_widgets.h"

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
        auto* head = f::row(*block, 12);
        f::text(*head, W(row["name"]), tok::TextPrimary, 14.0f, 600);
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
        for (size_t d = 0; d < row["digests"].size(); ++d) all += W(row["digests"][d]["digest"]) + L"  " + W(row["name"]) + L"\n";
    }
    std::wstring extension;
    for (size_t i = 0; i < m["algorithms"].size(); ++i) {
        if (m["algorithms"][i]["selected"].boolean()) {
            first_algorithm = W(m["algorithms"][i]["id"]);
            extension = W(m["algorithms"][i]["extension"]);
            break;
        }
    }
    if (!verify) {
        auto* save = add_button(c, L"Save…", [cx, folder, first_algorithm, extension] {
            if (folder.empty() || first_algorithm.empty()) return;
            const json::Value r = try_tool_call(cx.main, "checksum.save",
                                                "{\"path\":" + json_quote(folder + L"\\checksums" + extension) + ",\"algorithm\":" + json_quote(first_algorithm) + "}");
            tools_ui(cx.main).checksum_note = r["ok"].boolean() ? (r["note"].is_null() ? L"Saved checksums" + extension + L" beside the files." : W(r["note"])) : W(r["error"]);
            cx.main.open_dialog(L"checksum-show");
        });
        save->name = "ChecksumSave";
        save->set_enabled(m["canSave"].boolean());
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
    if (files.empty()) {
        return refusal(cx, verify ? L"Verify checksums" : L"Checksum", verify ? L"Select a checksum file (.sha256, .md5, .sfv ...) to verify." : L"Select one or more files to hash.");
    }
    std::wstring error;
    json::Value m;
    if (verify) {
        m = try_tool_call(cx.main, "checksum.verify", "{\"path\":" + json_quote(files[0].path) + "}", &error);
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
    auto* names = f::row(*c.body, 0);
    auto* l = f::text(*names, W(m["leftName"]), tok::TextPrimary, 14.0f, 600);
    l->flex = 1;
    auto* r = f::text(*names, W(m["rightName"]), tok::TextPrimary, 14.0f, 600);
    r->flex = 1;
    auto* verdict = f::text(*c.body, W(m["verdict"]), m["identical"].boolean() ? tok::TextLink : tok::TextSecondary, 13.5f);
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

std::unique_ptr<Panel> disk_usage_card(DialogContext& cx, const json::Value& m) {
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

    auto* progress = c.body->emplace<Panel>();
    progress->height = 2;

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
        tile_paths.push_back(ch["isDirectory"].boolean() && !ch["synthetic"].boolean() ? W(ch["path"]) : std::wstring());
    }
    if (folded > 0) {
        tiles.push_back({std::to_wstring(folded) + L" smaller items", L"", folded_weight, true});
        tile_paths.push_back({});
    }
    map->on_tile = [open_at, tile_paths](size_t i) {
        if (i < tile_paths.size() && !tile_paths[i].empty()) open_at(tile_paths[i]);
    };
    map->set_tiles(std::move(tiles));

    auto* list = left->emplace<tw::TableList>();
    list->name = "DiskUsageChildren";
    list->flex = 1;
    list->show_header = false;
    list->columns = {{L"Name", 0, true, false, false}, {L"Size", 110, false, true, false}};
    std::vector<tw::TableRow> rows;
    std::vector<std::wstring> row_paths;
    for (size_t i = 0; i < children.size(); ++i) {
        const json::Value& ch = children[i];
        tw::TableRow row;
        row.cells = {W(ch["name"]), W(ch["sizeText"])};
        // The bar is the measurement made visible, so an unmeasured row gets none: an empty bar is the
        // honest picture of a share nobody has established.
        row.bar = ch["unknown"].boolean() ? -1.0f : static_cast<float>(ch["fraction"].num());
        row.bar_slot = true;
        row.muted = ch["unknown"].boolean();
        rows.push_back(std::move(row));
        row_paths.push_back(ch["isDirectory"].boolean() && !ch["synthetic"].boolean() ? W(ch["path"]) : std::wstring());
    }
    list->set_rows(std::move(rows));
    list->on_activate = [open_at, row_paths](int i) {
        if (i >= 0 && static_cast<size_t>(i) < row_paths.size() && !row_paths[static_cast<size_t>(i)].empty()) open_at(row_paths[static_cast<size_t>(i)]);
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
    std::vector<tw::TableRow> frows;
    for (size_t i = 0; i < m["largest"].size(); ++i) {
        const json::Value& fl = m["largest"][i];
        tw::TableRow row;
        row.cells = {W(fl["name"]), W(fl["relativePath"]), W(fl["sizeText"])};
        frows.push_back(std::move(row));
    }
    files->empty_text = W(m["largestEmpty"]);
    files->set_rows(std::move(frows));

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

std::unique_ptr<Panel> disk_usage_for(DialogContext& cx, const std::wstring& root) {
    std::wstring error;
    const json::Value m = try_tool_call(cx.main, "diskusage.model", "{\"path\":" + json_quote(root) + "}", &error);
    if (m.is_null()) return refusal(cx, L"Disk usage", error);
    return disk_usage_card(cx, m);
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
        row.tint = change_colour(r["kind"].str());
        row.muted = r["deleted"].boolean();
        rows.push_back(std::move(row));
    }
    table->empty_text = W(m["emptyMessage"]);
    table->set_rows(std::move(rows));
    f::text(*c.body, W(m["summary"]), tok::TextSecondary, 13.0f)->name = "ChangesSummary";
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

std::unique_ptr<Panel> duplicates_card(DialogContext& cx) {
    Card c = make_card(L"Duplicate files", 820);
    DuplicatesState& d = cx.main.duplicates();
    if (!d.has) {
        f::note(*c.body, L"Nothing has been scanned yet. Find duplicates looks under the folder you are in.");
        add_button(c, L"Close", cx.close, true);
        return std::move(c.root);
    }
    const json::Value& groups = d.result["groups"];
    f::text(*c.body, std::to_wstring(groups.size()) + L" groups — " + format_bytes(static_cast<int64_t>(d.result["wastedBytes"].num())) + L" could be reclaimed.", tok::TextSecondary, 13.0f)
        ->name = "DuplicatesSummary";
    auto* keep = f::row(*c.body, 8);
    f::text(*keep, L"Keep one copy:", tok::TextSecondary, 13.0f);
    for (const char* strategy : {"newest", "oldest", "shallowest"}) {
        const std::string s = strategy;
        std::wstring label = core::wide(s);
        label[0] = static_cast<wchar_t>(std::towupper(label[0]));
        f::button(*keep, label, [cx, s] {
            DuplicatesState& st = cx.main.duplicates();
            st.ticked.clear();
            for (size_t g = 0; g < st.result["groups"].size(); ++g) {
                const size_t kept = static_cast<size_t>(st.result["groups"][g]["keep"][s].i64());
                for (size_t i = 0; i < st.result["groups"][g]["files"].size(); ++i) {
                    if (i != kept) st.ticked.insert({g, i});
                }
            }
            cx.main.open_dialog(L"duplicates");
        }, 110)->name = "Keep-" + s;
    }

    auto* table = c.body->emplace<tw::TableList>();
    table->name = "DuplicateRows";
    table->height = 330;
    table->columns = {{L"", 34, false, false, false}, {L"Name", 0, true, false, false}, {L"Folder", 0, true, false, false}, {L"Size", 100, false, true, false}};
    std::vector<tw::TableRow> rows;
    std::vector<std::pair<size_t, size_t>> index;  // per row: group, file (file == npos for a heading)
    for (size_t g = 0; g < groups.size(); ++g) {
        tw::TableRow h;
        h.heading = true;
        h.cells = {L"", std::to_wstring(groups[g]["files"].size()) + L" copies of " + format_bytes(static_cast<int64_t>(groups[g]["sizeBytes"].num())), L"", L""};
        rows.push_back(std::move(h));
        index.push_back({g, static_cast<size_t>(-1)});
        for (size_t i = 0; i < groups[g]["files"].size(); ++i) {
            const json::Value& file = groups[g]["files"][i];
            const bool ticked = d.ticked.count({g, i}) > 0;
            tw::TableRow row;
            row.cells = {ticked ? L"☑" : L"☐", W(file["name"]), file["dir"].str().empty() ? std::wstring(L"(this folder)") : W(file["dir"]),
                         format_bytes(static_cast<int64_t>(file["sizeBytes"].num()))};
            row.muted = !ticked;
            rows.push_back(std::move(row));
            index.push_back({g, i});
        }
    }
    table->set_rows(std::move(rows));
    table->on_activate = [cx, index](int i) {
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
    const size_t n = d.ticked.size();
    auto* remove = add_button(c, L"Remove ticked", [cx] { duplicates_ask_remove(cx.main); }, false, 150);
    remove->name = "RemoveTicked";
    remove->set_enabled(n > 0);
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
    f::text(*c.body, L"Make the right side match the left.", tok::TextPrimary, 14.0f);
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
        table->on_activate = [cx, keys](int i) {
            if (i < 0 || static_cast<size_t>(i) >= keys.size()) return;
            ToolsUi& st = tools_ui(cx.main);
            st.sync_touched = true;
            if (!st.sync_unticked.erase(keys[static_cast<size_t>(i)])) st.sync_unticked.insert(keys[static_cast<size_t>(i)]);
            cx.main.open_dialog(L"sync-preview-edit");
        };
    }

    if (!p["caveat"].is_null()) f::text(*c.body, W(p["caveat"]), tok::WarningForeground, 13.0f)->wrap = true;
    auto* deletes = f::check(*c.body, L"Also delete items that are only on the right", ui.sync_with_deletes, [cx](bool on) {
        tools_ui(cx.main).sync_with_deletes = on;
        cx.main.open_dialog(L"sync-preview-edit");
    });
    deletes->name = "SyncWithDeletes";
    deletes->set_enabled(!running);
    f::note(*c.body, L"Undo puts the whole sync back, and they go to the Recycle Bin on any volume that has one.");

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
        cx.close();
    }, true, 110);
    sync->name = "SyncRun";
    sync->set_enabled(p["canRun"].boolean());
    add_button(c, L"Cancel", cx.close);
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
        {L"changes", changes_start},
        {L"changes-show", changes_show},
        {L"duplicates", duplicates_card},
        {L"sync-preview", [](DialogContext& cx) { return sync_fresh(cx, false); }},
        {L"sync-preview-edit", [](DialogContext& cx) { return sync_card(cx, false); }},
        {L"sync-preview-running", [](DialogContext& cx) { return sync_fresh(cx, true); }},
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

// What is still duplicated: the removed copies leave their groups, and a group left with one copy is not a
// group any more. The ticks go with them.
void duplicates_prune(MainView& main, const std::vector<std::wstring>& removed) {
    std::set<std::string> gone;
    for (const auto& p : removed) gone.insert(core::utf8(p));
    DuplicatesState& d = main.duplicates();
    for (auto& kv : d.result.o) {
        if (kv.first != "groups") continue;
        std::vector<json::Value> kept;
        double wasted = 0;
        for (size_t g = 0; g < kv.second.a.size(); ++g) {
            json::Value group = kv.second.a[g];
            for (auto& member : group.o) {
                if (member.first != "files") continue;
                std::vector<json::Value> left;
                for (size_t i = 0; i < member.second.a.size(); ++i) {
                    if (!gone.count(member.second.a[i]["path"].str())) left.push_back(member.second.a[i]);
                }
                member.second.a = std::move(left);
            }
            if (group["files"].size() >= 2) {
                wasted += group["sizeBytes"].num() * static_cast<double>(group["files"].size() - 1);
                kept.push_back(std::move(group));
            }
        }
        kv.second.a = std::move(kept);
        for (auto& w : d.result.o) {
            if (w.first == "wastedBytes") w.second.n = wasted;
        }
    }
    d.ticked.clear();
}

// The ticked copies, deleted at once - for a scripted run, which stands in for someone who said yes to the
// confirmation. The window's button asks first (`duplicates_ask_remove`).
bool duplicates_remove(MainView& main, uint64_t* op) {
    const std::vector<std::wstring> paths = duplicates_removable(main);
    if (paths.empty()) return false;
    std::vector<BbStr> views;
    for (const auto& p : paths) views.push_back(core::s(p));
    uint64_t id = 0;
    core::check(bb_op_delete(main.core().app(), views.data(), static_cast<uint32_t>(views.size()), BB_DELETE_RECYCLE, &id), "duplicates-remove");
    if (op) *op = id;
    duplicates_prune(main, paths);
    return true;
}

// Remove ticked: the ordinary delete confirmation over the copies the rule allows, and back to this window
// afterwards with whatever was deleted gone from it. Spec: MainWindow.RemoveDuplicateCopies.
void duplicates_ask_remove(MainView& main) {
    const std::vector<std::wstring> paths = duplicates_removable(main);
    if (paths.empty()) {
        bb_status_set(main.core().app(), core::s(L"One copy of every file always stays - untick one in each group to remove the rest."));
        return;
    }
    main.ask_delete(paths, [&main, paths](bool confirmed) {
        if (confirmed) duplicates_prune(main, paths);
        main.defer([&main] { main.open_dialog(L"duplicates"); });
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
