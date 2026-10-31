// The dialogs that ask for a few fields and then act: rename (plain and advanced), compress, extract and
// the archive password. Each one plans through the core (`rename-preview`, `compress-defaults`) and carries
// out through the same exports the harness uses, so what the picture shows is what the verb will do.
#include "views/dialogs/ops_card.h"
#include "views/dialogs/file_pickers.h"

#include <filesystem>

namespace bb::views::ops {

using namespace bb::ui;
namespace f = forms;

namespace {

std::wstring w(const json::Value& v) { return core::wide(v.str()); }

std::wstring parent_dir(const std::wstring& path) {
    const size_t cut = path.find_last_of(L"\\/");
    return cut == std::wstring::npos ? std::wstring() : path.substr(0, cut);
}

std::wstring join_path(const std::wstring& dir, const std::wstring& name) {
    if (dir.empty()) return name;
    return dir.back() == L'\\' || dir.back() == L'/' ? dir + name : dir + L"\\" + name;
}

// ---- rename ------------------------------------------------------------------------------------------------

struct RenameState {
    MainView* main = nullptr;
    std::vector<std::wstring> paths;
    std::wstring suggestion, seeded;
    bool expanded = false;
    bool loading = false;
    bool ready = false;
    Panel* root = nullptr;
    Edit *name = nullptr, *find = nullptr, *replace = nullptr, *start = nullptr, *step = nullptr;
    CheckBox *regex = nullptr, *match_case = nullptr;
    ComboBox *scope = nullptr, *casebox = nullptr;
    TextBlock *prompt = nullptr, *problem = nullptr, *hint = nullptr, *preview_text = nullptr;
    f::ItemList* preview_list = nullptr;
    StackPanel* options = nullptr;
    Button *toggle = nullptr, *rename = nullptr;
    Widget* debounce = nullptr;
};

// Fires `fire` once, `ms` after the last kick: typing into a big selection re-plans when the typing
// pauses, not on every key. Spec: RenameDialog (its 200 ms DispatcherTimer).
class Debounce : public Widget {
public:
    std::function<void()> fire;
    void kick(unsigned ms) {
        if (root()) root()->start_timer(this, 1, ms);
    }
    void on_timer(int id) override {
        root()->stop_timer(this, id);
        if (fire) fire();
    }
    Size measure(Size) override { return {0, 0}; }
};

// Below this many items a plan is quick enough to follow every key (and a scripted run sees it at once).
constexpr size_t kDebounceAbove = 200;

bool numeric(const std::wstring& t) {
    if (t.empty()) return false;
    size_t i = (t[0] == L'-' || t[0] == L'+') ? 1 : 0;
    if (i >= t.size()) return false;
    for (; i < t.size(); ++i) {
        if (t[i] < L'0' || t[i] > L'9') return false;
    }
    return true;
}

std::string rule_json(const RenameState& st) {
    static const char* scopes[] = {"Stem", "Extension", "WholeName"};
    static const char* cases[] = {"AsIs", "Lower", "Upper", "Title", "Sentence"};
    const int sc = std::max(0, st.scope->selected()), ca = std::max(0, st.casebox->selected());
    auto num = [](const std::wstring& t, int fallback) { return numeric(t) ? _wtoi(t.c_str()) : fallback; };
    return "{\"template\":" + q(st.name->text()) + ",\"find\":" + q(st.find->text()) + ",\"replace\":" + q(st.replace->text()) +
           ",\"useRegex\":" + (st.regex->checked() ? "true" : "false") + ",\"matchCase\":" + (st.match_case->checked() ? "true" : "false") +
           ",\"scope\":\"" + scopes[sc] + "\",\"case\":\"" + cases[ca] + "\",\"counterStart\":" + std::to_string(num(st.start->text(), 1)) +
           ",\"counterStep\":" + std::to_string(num(st.step->text(), 1)) + "}";
}

void set_prompt(RenameState& st) {
    const size_t n = st.paths.size();
    if (st.expanded) st.prompt->set_text(n == 1 ? L"Name template:" : L"Name template for " + std::to_wstring(n) + L" items:");
    else st.prompt->set_text(n == 1 ? L"New name:" : L"New name for " + std::to_wstring(n) + L" items — they will be numbered:");
}

void replan(RenameState& st) {
    if (st.loading || !st.ready) return;
    const bool numbers_ok = numeric(st.start->text()) && numeric(st.step->text());
    const std::string args = "{\"paths\":" + qa(st.paths) + ",\"rule\":" + (st.expanded ? rule_json(st) : "null") +
                             ",\"pattern\":" + q(st.name->text()) + "}";
    json::Value p;
    try {
        p = query(st.main->core(), "rename-preview", args);
    } catch (const std::exception&) {
        return;
    }
    std::wstring problem = w(p["problem"]);
    if (st.expanded && !numbers_ok) problem = L"Counter start and step have to be whole numbers.";
    st.problem->set_text(problem);
    st.problem->set_visible(!problem.empty());
    const std::wstring hint = st.expanded ? w(p["hint"]) : L"";
    st.hint->set_text(hint);
    st.hint->set_visible(!hint.empty());
    const json::Value& rows = p["rows"];
    if (st.expanded) {
        std::vector<f::ListEntry> entries;
        for (size_t i = 0; i < rows.size(); ++i) {
            f::ListEntry e;
            e.text = w(rows[i]["name"]);
            const std::wstring prob = w(rows[i]["problem"]);
            e.detail = prob.empty() ? L"→  " + w(rows[i]["newName"]) : L"→  " + prob;
            e.muted = rows[i]["unchanged"].boolean();
            entries.push_back(std::move(e));
        }
        st.preview_list->set_entries(std::move(entries));
    } else {
        std::wstring text;
        const size_t work = static_cast<size_t>(p["work"].i64());
        if (st.paths.size() > 1 && work > 0) {
            size_t shown = 0;
            for (size_t i = 0; i < rows.size() && shown < 6; ++i) {
                if (rows[i]["unchanged"].boolean() || !rows[i]["problem"].is_null()) continue;
                text += (shown ? L"\n" : L"") + w(rows[i]["name"]) + L"  →  " + w(rows[i]["newName"]);
                ++shown;
            }
            if (work > 6) text += L"\n…and " + std::to_wstring(work - 6) + L" more";
        }
        st.preview_text->set_text(text);
        st.preview_text->set_visible(!text.empty());
    }
    st.rename->set_enabled(problem.empty() && p["hasWork"].boolean());
    st.root->request_layout();
}

void replan_soon(RenameState& st) {
    if (st.paths.size() > kDebounceAbove && st.debounce) static_cast<Debounce*>(st.debounce)->kick(200);
    else replan(st);
}

// The keyboard in the name box with the base name of one item selected, the whole box otherwise; again
// whenever the options open or close. Spec: RenameDialog.PlaceCaret.
void place_caret(RenameState& st) {
    if (st.name->root()) st.name->root()->set_focus(st.name);
    const std::wstring& n = st.name->text();
    if (st.expanded || st.paths.size() != 1) {
        st.name->select(0, n.size());
    } else {
        const size_t dot = n.find_last_of(L'.');
        st.name->select(0, dot == std::wstring::npos || dot == 0 ? n.size() : dot);
    }
}

void set_expanded(RenameState& st, bool expanded) {
    if (st.expanded == expanded) return;
    st.expanded = expanded;
    st.options->set_visible(expanded);
    st.preview_list->set_visible(expanded);
    st.preview_text->set_visible(false);
    st.toggle->text = expanded ? L"Fewer options" : L"More options";
    st.root->width = expanded ? 760.0f : 460.0f;
    set_prompt(st);
    st.loading = true;
    if (expanded) {
        // Seeded so that opening the panel never changes the pending result: a literal template is a
        // find/replace with nothing to act on.
        if (st.name->text() == st.suggestion) {
            const std::wstring escaped = [&] {
                std::wstring out;
                for (wchar_t ch : st.suggestion) {
                    out += ch;
                    if (ch == L'{' || ch == L'}') out += ch;
                }
                return out;
            }();
            st.seeded = st.paths.size() > 1 ? escaped + L" {n}{ext}" : L"{name}";
            st.name->set_text(st.seeded);
        }
    } else {
        if (!st.seeded.empty() && st.name->text() == st.seeded) st.name->set_text(st.suggestion);
        st.seeded.clear();
    }
    st.loading = false;
    replan(st);
    place_caret(st);
    st.root->request_layout();
}

}  // namespace

std::unique_ptr<Widget> rename_dialog(DialogContext& cx, bool expanded) {
    MainView& main = cx.main;
    auto st = std::make_shared<RenameState>();
    st->main = &main;
    for (const auto& r : main.selected_rows()) st->paths.push_back(r.path);
    Card c = make_card(L"Rename", 460);
    st->root = c.root.get();
    if (st->paths.empty()) {
        f::note(*c.body, L"Select something to rename first.");
        add_button(c, L"Close", cx.close, true);
        return std::move(c.root);
    }
    // The name the box starts on.
    try {
        const json::Value s = query(main.core(), "rename-preview", "{\"paths\":" + qa(st->paths) + ",\"rule\":null,\"pattern\":\"\"}");
        st->suggestion = w(s["suggestion"]);
    } catch (const std::exception&) {
    }
    BbOwnedStr saved{};
    const bool remembered = bb_settings_get(main.core().app(), core::s(L"advancedRenameExpanded"), &saved) == BB_OK && core::take(saved) == L"true";

    st->prompt = f::text(*c.body, L"", tok::TextPrimary, 14.0f);
    st->prompt->wrap = true;
    auto* debounce = c.body->emplace<Debounce>();
    debounce->set_visible(false);  // takes no room; its timer still fires
    debounce->fire = [st] { replan(*st); };
    st->debounce = debounce;
    st->name = f::field(*c.body, st->suggestion, -1, [st](const std::wstring&) { replan_soon(*st); });
    st->name->width = -1;
    st->name->name = "RenameName";
    st->toggle = f::button(*c.body, L"More options", [st] { set_expanded(*st, !st->expanded); }, 130);
    st->toggle->name = "RenameOptions";

    st->options = c.body->emplace<StackPanel>(Orientation::Vertical, 8.0f);
    st->options->name = "RenameOptionsPanel";
    auto changed = [st](const std::wstring&) { replan_soon(*st); };
    auto fr = f::row(*st->options, 8);
    f::row_label(*fr, L"Find", 110);
    st->find = f::field(*fr, L"", 200, changed);
    st->find->name = "RenameFind";
    f::row_label(*fr, L"Replace with", 90);
    st->replace = f::field(*fr, L"", 200, changed);
    st->replace->name = "RenameReplace";
    auto checks = f::row(*st->options, 16);
    st->regex = f::check(*checks, L"Regular expression", false, [st](bool) { replan(*st); });
    st->match_case = f::check(*checks, L"Match case", false, [st](bool) { replan(*st); });
    auto sr = f::row(*st->options, 8);
    f::row_label(*sr, L"Apply to", 110);
    st->scope = f::combo(*sr, {L"Name, without extension", L"Extension only", L"Name and extension"}, 0, 200, [st](int) { replan(*st); });
    f::row_label(*sr, L"Case", 90);
    st->casebox = f::combo(*sr, {L"As typed", L"lower case", L"UPPER CASE", L"Title Case", L"Sentence case"}, 0, 200, [st](int) { replan(*st); });
    auto cr = f::row(*st->options, 8);
    f::row_label(*cr, L"Counter start", 110);
    st->start = f::field(*cr, L"1", 60, changed);
    f::row_label(*cr, L"step", 40);
    st->step = f::field(*cr, L"1", 60, changed);
    f::text(*st->options,
            L"Name tokens: {name} {base} {ext} {parent} {n} {n:000} {modified} {modified:yyyy-MM} — {ext} brings its own dot, and {{ writes a literal brace.",
            tok::TextSecondary, 11.0f)->wrap = true;
    st->options->set_visible(false);

    st->problem = f::text(*c.body, L"", tok::ErrorForeground, 13.0f);
    st->problem->wrap = true;
    st->problem->set_visible(false);
    st->hint = f::text(*c.body, L"", tok::WarningForeground, 13.0f);
    st->hint->wrap = true;
    st->hint->set_visible(false);
    st->preview_text = f::text(*c.body, L"", tok::TextSecondary, 13.0f);
    st->preview_text->set_visible(false);
    st->preview_text->name = "RenamePreview";
    st->preview_list = c.body->emplace<f::ItemList>();
    st->preview_list->height = 190;
    st->preview_list->name = "RenamePreviewList";
    st->preview_list->set_visible(false);

    add_button(c, L"Cancel", cx.close);
    st->rename = add_button(c, L"Rename", [st, close = cx.close] {
        replan(*st);  // disk may have changed while the dialog sat open
        if (!st->rename->flags.enabled) return;
        core::Core& core = st->main->core();
        std::vector<BbStr> views;
        for (const auto& p : st->paths) views.push_back(core::s(p));
        uint64_t op = 0;
        BbStatus started = BB_OK;
        if (st->expanded) {
            const std::wstring rule = core::wide(rule_json(*st));
            started = bb_op_rename_rule(core.app(), views.data(), static_cast<uint32_t>(views.size()), core::s(rule), &op);
        } else {
            started = bb_op_rename(core.app(), views.data(), static_cast<uint32_t>(views.size()), core::s(st->name->text()), &op);
        }
        // An archive edit the core refuses is said in a message, not only on the status bar.
        // Spec: DirectoryTabView.RunArchiveEditAsync.
        const std::wstring refused = started == BB_ERR ? core::last_error() : std::wstring();
        bb_settings_set(core.app(), core::s(L"advancedRenameExpanded"), core::s(st->expanded ? L"true" : L"false"));
        if (st->expanded) {
            // Kept without its text. Spec: RenameDialog.SaveOptions.
            const std::string kept = rule_json(*st);
            const std::string keep_knobs = "{\"template\":\"\",\"find\":\"\",\"replace\":\"\"" + kept.substr(kept.find(",\"useRegex\""));
            bb_settings_set(core.app(), core::s(L"advancedRename"), core::s(core::wide(keep_knobs)));
        }
        if (!refused.empty()) st->main->show_message(L"Archive", refused);  // deferred, so before close() frees this handler
        close();
    }, true);
    st->rename->name = "RenameOk";

    // The knobs from last time, not the text: a regular expression from last week waiting behind F2 is a trap,
    // but setting the counter again every time is a nuisance. Spec: RenameDialog.LoadOptions.
    {
        BbOwnedStr ar{};
        if (bb_settings_get(main.core().app(), core::s(L"advancedRename"), &ar) == BB_OK) {
            json::Value v;
            try {
                v = json::parse(core::utf8(core::take(ar)));
            } catch (const std::exception&) {
            }
            if (v.is_object()) {
                static const char* scopes[] = {"Stem", "Extension", "WholeName"};
                static const char* cases[] = {"AsIs", "Lower", "Upper", "Title", "Sentence"};
                st->regex->set_checked(v["useRegex"].boolean());
                st->match_case->set_checked(v["matchCase"].boolean());
                for (int i = 0; i < 3; ++i) if (v["scope"].str() == scopes[i]) st->scope->set_selected(i);
                for (int i = 0; i < 5; ++i) if (v["case"].str() == cases[i]) st->casebox->set_selected(i);
                if (!v["counterStart"].is_null()) st->start->set_text(std::to_wstring(v["counterStart"].i64()));
                if (!v["counterStep"].is_null()) st->step->set_text(std::to_wstring(v["counterStep"].i64()));
            }
        }
    }
    st->ready = true;
    set_prompt(*st);
    if (expanded || remembered) set_expanded(*st, true);
    else replan(*st);
    place_caret(*st);
    return std::move(c.root);
}

// ---- compress ------------------------------------------------------------------------------------------------

std::unique_ptr<Widget> compress_dialog(DialogContext& cx) {
    MainView& main = cx.main;
    const auto rows = main.selected_rows();
    const std::wstring folder = main.core().tab_text(main.active_tab(), BB_TEXT_PATH);
    std::vector<std::wstring> sources;
    for (const auto& r : rows) sources.push_back(r.path);
    if (sources.empty()) sources.push_back(folder);
    // Compressing the folder itself puts the archive beside it, not inside what it archives.
    std::wstring directory = folder;
    if (rows.empty()) {
        const std::wstring trimmed = folder.size() > 3 && folder.back() == L'\\' ? folder.substr(0, folder.size() - 1) : folder;
        const size_t cut = trimmed.find_last_of(L'\\');
        if (cut != std::wstring::npos && trimmed.size() > 3) directory = trimmed.substr(0, cut < 3 ? 3 : cut);
    }
    const json::Value defaults = query(main.core(), "compress-defaults", "{\"sources\":" + qa(sources) + ",\"directory\":" + q(folder) + "}");
    Card c = make_card(L"Compress", 480);
    std::wstring what;
    for (size_t i = 0; i < rows.size() && i < 4; ++i) what += (i ? L"\n" : L"") + rows[i].name;
    if (rows.size() > 4) what += L"\n\u2026and " + std::to_wstring(rows.size() - 4) + L" more";
    f::text(*c.body, rows.empty() ? L"The whole folder" : what, tok::TextSecondary, 13.0f)->wrap = true;
    f::text(*c.body, L"Archive name", tok::TextSecondary, 13.0f);
    auto* name = f::field(*c.body, w(defaults["name"]), -1, nullptr);
    name->width = -1;
    name->name = "CompressName";
    f::text(*c.body, L"Format", tok::TextSecondary, 13.0f);
    std::vector<std::wstring> labels;
    const json::Value& formats = defaults["formats"];
    for (size_t i = 0; i < formats.size(); ++i) labels.push_back(w(formats[i]["label"]));
    auto* format = f::combo(*c.body, labels, 0, 200, nullptr);
    format->name = "CompressFormat";
    f::text(*c.body, L"Compression", tok::TextSecondary, 13.0f);
    auto* level = f::radios(*c.body, {L"Store (no compression)", L"Normal", L"Maximum"}, 1, nullptr);
    level->name = "CompressLevel";
    f::text(*c.body, L"Where", tok::TextSecondary, 13.0f);
    auto* r = f::row(*c.body);
    auto* dest = f::field(*r, directory, 330, nullptr);
    dest->name = "CompressWhere";
    auto* result = f::text(*c.body, L"", tok::TextMuted, 12.5f);
    result->name = "CompressResult";
    result->wrap = true;
    auto* problem = f::text(*c.body, L"", tok::ErrorForeground, 13.0f);
    problem->name = "CompressProblem";
    problem->wrap = true;
    add_button(c, L"Cancel", cx.close);
    Button* ok = add_button(c, L"Compress", nullptr, true);
    ok->name = "CompressOk";
    core::Core* core = &main.core();
    auto target = std::make_shared<std::wstring>();
    auto check = [core, formats, name, format, level, dest, result, problem, ok, target] {
        const json::Value& f_ = formats[static_cast<size_t>(std::max(0, format->selected()))];
        // A format with no levels (tar) says so by greying the choice rather than ignoring it.
        const bool levels = f_["supportsLevel"].boolean(true);
        for (size_t i = 0; i < 3; ++i) level->set_item_enabled(i, levels);
        const json::Value r = query(*core, "compress-check", "{\"name\":" + q(name->text()) + ",\"directory\":" + q(dest->text()) + ",\"suffix\":" + q(w(f_["suffix"])) + "}");
        const bool fine = r["problem"].is_null();
        *target = w(r["path"]);
        result->set_text(fine ? L"Creates: " + *target : L"");
        problem->set_text(fine ? L"" : w(r["problem"]));
        ok->set_enabled(fine);
        result->invalidate();
        problem->invalidate();
        ok->invalidate();
    };
    f::button(*r, L"Browse\u2026", [&main, dest, check] {
        if (!main.shows_dialogs()) return;
        PickPath pick;
        pick.title = L"Put the archive in";
        pick.folder = dest->text();
        if (const auto chosen = pick_folder(main.window().hwnd(), pick)) {
            dest->set_text(*chosen);
            check();
        }
    })->name = "CompressBrowse";
    name->on_changed = [check](const std::wstring&) { check(); };
    dest->on_changed = [check](const std::wstring&) { check(); };
    format->on_change = [check](int) { check(); };
    check();
    ok->on_click = [&main, formats, sources, format, level, target, check, ok, close = cx.close] {
        check();  // the folder may have gained a file of that name while this sat open
        if (!ok->flags.enabled) return;
        const json::Value& f_ = formats[static_cast<size_t>(std::max(0, format->selected()))];
        static const char* levels[] = {"store", "normal", "maximum"};
        // A plan the core refuses is said in a message, not lost. Spec: DirectoryTabView.CompressAsync.
        try {
            run(main.core(), "compress",
                "{\"sources\":" + qa(sources) + ",\"target\":" + q(*target) + ",\"format\":" + q(w(f_["label"])) +
                    ",\"level\":\"" + levels[std::max(0, level->selected())] + "\"}");
        } catch (const std::exception& e) {
            main.show_message(L"Compress", core::message_of(e));  // deferred, so before close() frees this handler
            close();
            return;
        }
        close();
    };
    name->on_commit = [ok] {
        if (ok->flags.enabled && ok->on_click) ok->on_click();
    };
    return std::move(c.root);
}

// ---- extract -------------------------------------------------------------------------------------------------

// What to extract: the archive the tab is inside (and the selection in it, or everything), or the one archive
// selected in an ordinary folder. Spec: DirectoryTabView.ExtractTarget.
ExtractSubject extract_subject(MainView& main) {
    ExtractSubject s;
    const auto rows = main.selected_rows();
    const std::wstring tab_path = main.core().tab_text(main.active_tab(), BB_TEXT_PATH);
    const json::Value inside = query(main.core(), "archive-of", core::utf8(tab_path));
    if (!inside.is_null()) {
        s.archive = w(inside["archive"]);
        s.inside = true;
        for (const auto& r : rows) s.names.push_back(r.path);
    } else if (rows.size() == 1) {
        const json::Value sel = query(main.core(), "archive-of", core::utf8(rows[0].path));
        if (!sel.is_null()) s.archive = w(sel["archive"]);
    }
    return s;
}

void run_extract(MainView& main, const ExtractSubject& s, const std::wstring& destination, bool skip) {
    std::string arg = "{\"tab\":" + std::to_string(main.active_tab()) + ",\"names\":" + qa(s.names) + ",\"destination\":" + q(destination) +
                      ",\"conflict\":\"" + (skip ? "skip" : "keep-both") + "\"";
    // An archive file selected in an ordinary folder is its own subject.
    if (!s.inside) arg += ",\"archive\":" + q(s.archive);
    run(main.core(), "extract", arg + "}");
}

std::wstring extract_destination(MainView& main, const std::wstring& archive) {
    return w(query(main.core(), "extract-destination", "{\"archive\":" + q(archive) + "}")["destination"]);
}

std::unique_ptr<Widget> extract_dialog(DialogContext& cx) {
    MainView& main = cx.main;
    const ExtractSubject subject = extract_subject(main);
    Card c = make_card(L"Extract", 480);
    const std::wstring archive_name = subject.archive.empty() ? L"the archive" : subject.archive.substr(subject.archive.find_last_of(L"\\/") + 1);
    f::text(*c.body, subject.names.empty() ? L"Extract everything from " + archive_name + L"."
                                           : L"Extract " + std::to_wstring(subject.names.size()) + L" selected item(s) from " + archive_name + L".",
            tok::TextPrimary, 13.5f)->wrap = true;
    f::text(*c.body, L"Into", tok::TextSecondary, 13.0f);
    auto* r = f::row(*c.body);
    auto* dest = f::field(*r, subject.archive.empty() ? std::wstring() : extract_destination(main, subject.archive), 330, nullptr);
    dest->name = "ExtractDestination";
    auto* problem = f::text(*c.body, L"", tok::ErrorForeground, 13.0f);
    problem->name = "ExtractProblem";
    problem->wrap = true;
    f::text(*c.body, L"If a file is already there", tok::TextSecondary, 13.0f);
    auto* conflict = f::radios(*c.body, {L"Keep both (number the new one)", L"Skip it (keep what is there)"}, 0, nullptr);
    conflict->name = "ExtractConflict";
    add_button(c, L"Cancel", cx.close);
    Button* go = add_button(c, L"Extract", nullptr, true);
    go->name = "ExtractOk";
    core::Core* core = &main.core();
    const bool has_archive = !subject.archive.empty();
    auto check = [core, dest, problem, go, has_archive] {
        const json::Value r = query(*core, "extract-check", "{\"destination\":" + q(dest->text()) + "}");
        problem->set_text(r["problem"].is_null() ? L"" : w(r["problem"]));
        go->set_enabled(has_archive && r["problem"].is_null());
        problem->invalidate();
        go->invalidate();
    };
    f::button(*r, L"Browse\u2026", [&main, dest, check] {
        if (!main.shows_dialogs()) return;
        PickPath pick;
        pick.title = L"Extract to";
        pick.folder = dest->text();
        if (const auto chosen = pick_folder(main.window().hwnd(), pick)) {
            dest->set_text(*chosen);
            check();
        }
    })->name = "ExtractBrowse";
    dest->on_changed = [check](const std::wstring&) { check(); };
    check();
    go->on_click = [&main, subject, dest, conflict, check, go, close = cx.close] {
        check();
        if (!go->flags.enabled) return;
        // The executor makes the folder as it writes, so a refused plan leaves nothing behind.
        std::wstring target = dest->text();
        while (target.size() > 3 && (target.back() == L' ' || target.back() == L'\\')) target.pop_back();
        // Spec: DirectoryTabView.ExtractAsync (a refused plan is a warning message).
        try {
            run_extract(main, subject, target, conflict->selected() == 1);
        } catch (const std::exception& e) {
            main.show_message(L"Extract", core::message_of(e));  // deferred, so before close() frees this handler
            close();
            return;
        }
        close();
    };
    return std::move(c.root);
}

// ---- the archive's password -----------------------------------------------------------------------------------

std::unique_ptr<Widget> password_dialog(DialogContext& cx) {
    MainView& main = cx.main;
    const std::wstring tab_path = main.core().tab_text(main.active_tab(), BB_TEXT_PATH);
    const json::Value of = query(main.core(), "archive-of", core::utf8(tab_path));
    const std::wstring archive = of.is_null() ? std::wstring() : w(of["archive"]);
    const std::wstring name = archive.empty() ? L"This archive" : archive.substr(archive.find_last_of(L"\\/") + 1);
    Card c = make_card(L"Password", 440);
    f::text(*c.body, name + L" is protected. Enter its password to read it.", tok::TextPrimary, 13.5f)->wrap = true;
    // A wrong one comes straight back as the same locked archive, so the next ask says so. Spec: UnlockAsync.
    if (!of.is_null() && of["refused"].boolean()) f::text(*c.body, L"That password did not work.", tok::ErrorForeground, 13.0f)->name = "ArchivePasswordRefused";
    f::note(*c.body, L"Remembered until BertBrowser closes, and never written to disk.");
    f::text(*c.body, L"Password", tok::TextSecondary, 13.0f);
    auto* pw = f::field(*c.body, L"", -1, nullptr);
    pw->width = -1;
    pw->password = true;
    pw->name = "ArchivePassword";
    f::check(*c.body, L"Show the password", false, [pw](bool on) {
        pw->password = !on;
        pw->invalidate();
    });
    add_button(c, L"Cancel", cx.close);
    Button* unlock = add_button(c, L"Unlock", nullptr, true);
    unlock->name = "ArchiveUnlock";
    unlock->set_enabled(false);
    pw->on_changed = [unlock](const std::wstring& t) {
        unlock->set_enabled(!t.empty());
        unlock->invalidate();
    };
    unlock->on_click = [&main, pw, unlock, close = cx.close] {
        if (!unlock->flags.enabled) return;
        run(main.core(), "unlock", "{\"tab\":" + std::to_string(main.active_tab()) + ",\"password\":" + q(pw->text()) + "}");
        close();
    };
    pw->on_commit = [unlock] {
        if (unlock->on_click) unlock->on_click();
    };
    return std::move(c.root);
}

}  // namespace bb::views::ops
