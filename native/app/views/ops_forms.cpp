// The dialogs that ask for a few fields and then act: rename (plain and advanced), compress, extract and
// the archive password. Each one plans through the core (`rename-preview`, `compress-defaults`) and carries
// out through the same exports the harness uses, so what the picture shows is what the verb will do.
#include "views/ops_card.h"

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
};

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
    st->name = f::field(*c.body, st->suggestion, -1, [st](const std::wstring&) { replan(*st); });
    st->name->width = -1;
    st->name->name = "RenameName";
    st->toggle = f::button(*c.body, L"More options", [st] { set_expanded(*st, !st->expanded); }, 130);
    st->toggle->name = "RenameOptions";

    st->options = c.body->emplace<StackPanel>(Orientation::Vertical, 8.0f);
    st->options->name = "RenameOptionsPanel";
    auto changed = [st](const std::wstring&) { replan(*st); };
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
        if (st->expanded) {
            const std::wstring rule = core::wide(rule_json(*st));
            bb_op_rename_rule(core.app(), views.data(), static_cast<uint32_t>(views.size()), core::s(rule), &op);
        } else {
            bb_op_rename(core.app(), views.data(), static_cast<uint32_t>(views.size()), core::s(st->name->text()), &op);
        }
        bb_settings_set(core.app(), core::s(L"advancedRenameExpanded"), core::s(st->expanded ? L"true" : L"false"));
        close();
    }, true);
    st->rename->name = "RenameOk";

    st->ready = true;
    set_prompt(*st);
    if (expanded || remembered) set_expanded(*st, true);
    else replan(*st);
    // The caret: the base name of one item is selected, the whole box otherwise.
    if (st->expanded || st->paths.size() != 1) {
        st->name->select(0, st->name->text().size());
    } else {
        const std::wstring& n = st->name->text();
        const size_t dot = n.find_last_of(L'.');
        st->name->select(0, dot == std::wstring::npos || dot == 0 ? n.size() : dot);
    }
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
    const json::Value defaults = query(main.core(), "compress-defaults", "{\"sources\":" + qa(sources) + ",\"directory\":" + q(folder) + "}");
    Card c = make_card(L"Compress", 480);
    std::wstring what;
    for (size_t i = 0; i < rows.size() && i < 4; ++i) what += (i ? L"\n" : L"") + rows[i].name;
    if (rows.size() > 4) what += L"\n…and " + std::to_wstring(rows.size() - 4) + L" more";
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
    auto* level = f::radios(*c.body, {L"Fastest", L"Normal", L"Smallest"}, 1, nullptr);
    level->name = "CompressLevel";
    f::text(*c.body, L"Where", tok::TextSecondary, 13.0f);
    auto* r = f::row(*c.body);
    auto* dest = f::field(*r, folder, 330, nullptr);
    dest->name = "CompressWhere";
    f::button(*r, L"Browse…", nullptr);
    add_button(c, L"Cancel", cx.close);
    add_button(c, L"Compress", [&main, formats, sources, name, format, level, dest, close = cx.close] {
        const json::Value& f_ = formats[static_cast<size_t>(std::max(0, format->selected()))];
        std::wstring file = name->text();
        const std::wstring suffix = w(f_["suffix"]);
        if (file.size() < suffix.size() || _wcsicmp(file.c_str() + file.size() - suffix.size(), suffix.c_str()) != 0) file += suffix;
        static const char* levels[] = {"store", "normal", "maximum"};
        run(main.core(), "compress",
            "{\"sources\":" + qa(sources) + ",\"target\":" + q(join_path(dest->text(), file)) + ",\"format\":" + q(w(f_["label"])) +
                ",\"level\":\"" + levels[std::max(0, level->selected())] + "\"}");
        close();
    }, true)->name = "CompressOk";
    return std::move(c.root);
}

// ---- extract -------------------------------------------------------------------------------------------------

std::unique_ptr<Widget> extract_dialog(DialogContext& cx) {
    MainView& main = cx.main;
    const auto rows = main.selected_rows();
    const std::wstring tab_path = main.core().tab_text(main.active_tab(), BB_TEXT_PATH);
    // Standing on an archive file in an ordinary folder, or inside one: either way it is one archive.
    std::wstring archive_file;
    std::vector<std::wstring> names;
    const json::Value inside = query(main.core(), "archive-of", core::utf8(tab_path));
    if (!inside.is_null()) {
        archive_file = w(inside["archive"]);
        for (const auto& r : rows) names.push_back(r.path);
    } else if (rows.size() == 1) {
        const json::Value sel = query(main.core(), "archive-of", core::utf8(rows[0].path));
        if (!sel.is_null()) archive_file = w(sel["archive"]);
    }
    Card c = make_card(L"Extract", 480);
    const std::wstring stem = [&] {
        std::wstring n = archive_file.empty() ? L"Extracted" : archive_file.substr(archive_file.find_last_of(L"\\/") + 1);
        for (const wchar_t* ext : {L".tar.gz", L".tar.bz2", L".tgz", L".zip", L".7z", L".rar", L".tar"}) {
            const size_t len = wcslen(ext);
            if (n.size() > len && _wcsicmp(n.c_str() + n.size() - len, ext) == 0) return n.substr(0, n.size() - len);
        }
        const size_t dot = n.find_last_of(L'.');
        return dot == std::wstring::npos || dot == 0 ? n : n.substr(0, dot);
    }();
    f::text(*c.body, archive_file.empty() ? L"The archive" : archive_file.substr(archive_file.find_last_of(L"\\/") + 1), tok::TextSecondary, 13.0f);
    f::text(*c.body, L"Extract to", tok::TextSecondary, 13.0f);
    const std::wstring base = archive_file.empty() ? tab_path : parent_dir(archive_file);
    auto* where = f::radios(*c.body, {L"A folder named after the archive", L"This folder"}, 0, nullptr);
    where->name = "ExtractWhere";
    f::text(*c.body, L"Destination", tok::TextSecondary, 13.0f);
    auto* r = f::row(*c.body);
    auto* dest = f::field(*r, base, 330, nullptr);
    dest->name = "ExtractDestination";
    f::button(*r, L"Browse…", nullptr);
    f::text(*c.body, L"If a file is already there", tok::TextSecondary, 13.0f);
    auto* conflict = f::combo(*c.body, {L"Keep both", L"Skip it"}, 0, 240, nullptr);
    conflict->name = "ExtractConflict";
    add_button(c, L"Cancel", cx.close);
    auto* go = add_button(c, L"Extract", [&main, archive_file, names, where, dest, conflict, stem, close = cx.close] {
        std::wstring target = dest->text();
        if (where->selected() == 0) target = join_path(target, stem);
        std::error_code ec;
        std::filesystem::create_directories(target, ec);
        const std::string arg = "{\"tab\":" + std::to_string(main.active_tab()) + ",\"names\":" + qa(names) + ",\"destination\":" + q(target) +
                                ",\"conflict\":\"" + (conflict->selected() == 1 ? "skip" : "keep-both") + "\"}";
        // An archive file selected in an ordinary folder is its own subject.
        if (names.empty() && !archive_file.empty()) {
            std::string with = arg;
            with.insert(1, "\"archive\":" + q(archive_file) + ",");
            run(main.core(), "extract", with);
        } else {
            run(main.core(), "extract", arg);
        }
        close();
    }, true);
    go->name = "ExtractOk";
    go->set_enabled(!archive_file.empty());
    return std::move(c.root);
}

// ---- the archive's password -----------------------------------------------------------------------------------

std::unique_ptr<Widget> password_dialog(DialogContext& cx) {
    MainView& main = cx.main;
    Card c = make_card(L"This archive is protected", 440);
    f::note(*c.body, L"Enter the password to open it. It is kept for this session only.");
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
    add_button(c, L"Unlock", [&main, pw, close = cx.close] {
        run(main.core(), "unlock", "{\"tab\":" + std::to_string(main.active_tab()) + ",\"password\":" + q(pw->text()) + "}");
        close();
    }, true)->name = "ArchiveUnlock";
    return std::move(c.root);
}

}  // namespace bb::views::ops
