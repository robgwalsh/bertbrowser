#include "views/ops_card.h"
#include "views/tool_state.h"

#include "gfx/icons.gen.h"
#include "views/command_ui.h"

#include <set>

namespace bb::views::ops {

using namespace bb::ui;
namespace f = forms;

// ---- helpers ---------------------------------------------------------------------------------------------

std::string q(const std::wstring& text) {
    const std::string u = core::utf8(text);
    std::string out = "\"";
    for (const unsigned char c : u) {
        switch (c) {
            case '\\': out += "\\\\"; break;
            case '"': out += "\\\""; break;
            case '\n': out += "\\n"; break;
            case '\r': out += "\\r"; break;
            case '\t': out += "\\t"; break;
            default:
                if (c < 0x20) {
                    char buf[8];
                    snprintf(buf, sizeof(buf), "\\u%04x", c);
                    out += buf;
                } else {
                    out += static_cast<char>(c);
                }
        }
    }
    return out + "\"";
}

std::string qa(const std::vector<std::wstring>& items) {
    std::string out = "[";
    for (size_t i = 0; i < items.size(); ++i) out += (i ? "," : "") + q(items[i]);
    return out + "]";
}

json::Value query(core::Core& c, const std::string& what, const std::string& arg) {
    return model([&](BbOwnedStr* o) {
        return bb_ops_query(c.app(), core::s(core::wide(what)), core::s(core::wide(arg)), o);
    }, "ops query");
}

json::Value run(core::Core& c, const std::string& verb, const std::string& arg) {
    BbOwnedStr out{};
    core::check(bb_ops_do(c.app(), core::s(core::wide(verb)), core::s(core::wide(arg)), &out), "ops verb");
    const std::string text = core::utf8(core::take(out));
    return text.empty() ? json::Value{} : json::parse(text);
}

Card make_card(const std::wstring& title, float width, std::unique_ptr<StackPanel> body) {
    Card c;
    c.root = std::make_unique<GuardedPanel>();
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
    auto* spacer = foot->emplace<Widget>();
    spacer->flex = 1;
    c.buttons = foot;
    if (body) {
        c.body = dock->add(std::move(body));
    } else {
        c.body = dock->emplace<StackPanel>(Orientation::Vertical, 8.0f);
    }
    c.body->dock = Dock::Fill;
    c.body->pad = 16;
    return c;
}

Button* add_button(Card& c, const wchar_t* label, std::function<void()> fn, bool primary, float width) {
    auto* b = f::button(*c.buttons, label, std::move(fn), width);
    b->primary = primary;
    return b;
}

LiveBody::LiveBody(MainView& main, Slot slot, std::function<void(LiveBody&)> fill)
    : StackPanel(Orientation::Vertical, 8.0f), link_(std::make_shared<Link>(Link{this})), fill_(std::move(fill)) {
    (main.*slot) = [link = link_] {
        if (link->body) link->body->refresh();
    };
    fill_(*this);
}

LiveBody::~LiveBody() { link_->body = nullptr; }

void LiveBody::refresh() {
    while (!children().empty()) remove_child(children().back().get());
    fill_(*this);
    if (after) after();
    request_layout();
    invalidate();
}

namespace {

std::wstring w(const json::Value& v) { return core::wide(v.str()); }

std::wstring describe_path(const std::wstring& path) {
    const size_t cut = path.find_last_of(L"\\/");
    if (cut == std::wstring::npos || cut + 1 >= path.size()) return path;
    return path.substr(cut + 1) + L"  —  " + path.substr(0, cut);
}

TextBlock* dim(Widget& parent, const std::wstring& t, Token fg = tok::TextSecondary) {
    return f::text(parent, t, fg, 12.0f);
}

}  // namespace

// ---- the Transfers window --------------------------------------------------------------------------------

std::unique_ptr<Widget> transfer_dialog(DialogContext& cx) {
    MainView& main = cx.main;
    main.sync_transfer();
    auto fill = [&main](LiveBody& body) {
        const json::Value& m = main.queue_model();
        if (!m["active"].boolean()) {
            f::note(body, L"Nothing is being copied or moved.");
            return;
        }
        const json::Value& v = m["view"];
        auto* head = f::text(body, w(v["headline"]), tok::TextPrimary, 14.0f);
        head->name = "TransferHeadline";
        auto* bar = body.emplace<ProgressBar>();
        bar->height = 6;
        bar->set_value(v["fraction"].num());
        bar->set_indeterminate(v["indeterminate"].boolean());
        auto* nums = f::row(body, 12);
        dim(*nums, w(v["bytesText"]))->flex = 0;
        dim(*nums, w(v["rateText"]));
        auto* eta = dim(*nums, w(v["etaText"]));
        eta->flex = 1;
        eta->align = gfx::TextAlign::Trailing;

        // What is waiting behind the job in flight, in the order it will run.
        if (!m["waitingText"].str().empty()) {
            f::gap(body, 4);
            f::text(body, L"Queued", tok::TextPrimary, 14.0f, 600);
            const json::Value& jobs = m["jobs"];
            auto* list = body.emplace<StackPanel>(Orientation::Vertical, 2.0f);
            list->name = "TransferQueue";
            for (size_t i = 0; i < jobs.size(); ++i) {
                const json::Value& j = jobs[i];
                const uint64_t id = static_cast<uint64_t>(j["id"].i64());
                auto* row = f::row(*list, 6);
                row->height = 26;
                auto* label = f::text(*row, w(j["kind"]) + L" " + w(j["description"]), tok::TextPrimary, 13.0f);
                label->flex = 1;
                auto* size = dim(*row, w(j["sizeText"]));
                size->width = 70;
                size->align = gfx::TextAlign::Trailing;
                auto* state = dim(*row, w(j["stateText"]));
                state->width = 76;
                state->align = gfx::TextAlign::Trailing;
                const std::string state_word = j["state"].str();
                core::Core* core = &main.core();
                if (state_word == "waiting") {
                    auto* up = f::button(*row, L"↑", [core, id] { run(*core, "queue-move", "{\"id\":" + std::to_string(id) + ",\"delta\":-1}"); }, 28);
                    up->set_enabled(j["canUp"].boolean());
                    up->name = "QueueUp" + std::to_string(id);
                    auto* down = f::button(*row, L"↓", [core, id] { run(*core, "queue-move", "{\"id\":" + std::to_string(id) + ",\"delta\":1}"); }, 28);
                    down->set_enabled(j["canDown"].boolean());
                    down->name = "QueueDown" + std::to_string(id);
                }
                if (state_word == "waiting" || state_word == "running") {
                    auto* cancel = f::button(*row, L"Cancel", [core, id] { run(*core, "queue-cancel", std::to_string(id)); }, 64);
                    cancel->name = "QueueCancel" + std::to_string(id);
                }
            }
        }

        // Every item of the job in flight, so a batch says which ones are through.
        const json::Value& items = v["items"];
        if (items.size() > 0) {
            f::gap(body, 4);
            auto* scroll = body.emplace<ScrollViewer>();
            scroll->height = std::min(180.0f, 38.0f * static_cast<float>(items.size()) + 4);
            scroll->style.bg = tok::InputBackground;
            scroll->style.border = tok::BorderSubtle;
            auto* col = scroll->emplace<StackPanel>(Orientation::Vertical, 0.0f);
            col->pad = 4;
            const size_t shown = std::min<size_t>(items.size(), 200);
            for (size_t i = 0; i < shown; ++i) {
                const json::Value& it = items[i];
                auto* row = f::row(*col, 8);
                auto* two = row->emplace<StackPanel>(Orientation::Vertical, 0.0f);
                two->flex = 1;
                f::text(*two, w(it["name"]), tok::TextPrimary, 13.0f);
                dim(*two, w(it["destination"]), tok::TextMuted)->style.font.size = 11.0f;
                const int s = static_cast<int>(it["state"].i64());
                auto* st = dim(*row, s == 2 ? L"done" : s == 1 ? L"in progress" : L"waiting");
                st->width = 76;
                st->align = gfx::TextAlign::Trailing;
            }
            if (items.size() > shown) dim(*col, L"…and " + std::to_wstring(items.size() - shown) + L" more", tok::TextMuted);
        }
    };
    auto body = std::make_unique<LiveBody>(main, &MainView::on_transfer_changed, fill);
    LiveBody* live = body.get();
    Card c = make_card(L"Transfers", 560, std::move(body));
    Button* pause = add_button(c, L"Pause", [&main, cx] {
        const bool paused = main.queue_model()["paused"].boolean();
        run(main.core(), paused ? "queue-resume" : "queue-pause");
    });
    pause->name = "TransferPause";
    add_button(c, L"Close", cx.close);
    Button* cancel = add_button(c, L"Cancel", [&main] { run(main.core(), "cancel-running"); });
    cancel->name = "TransferCancelRunning";
    live->after = [&main, pause, cancel] {
        const json::Value& m = main.queue_model();
        pause->text = m["paused"].boolean() ? L"Resume" : L"Pause";
        pause->set_enabled(m["canPause"].boolean() || m["canResume"].boolean());
        cancel->set_enabled(m["canCancel"].boolean());
    };
    live->after();
    return std::move(c.root);
}

// ---- a clash, one answer at a time ----------------------------------------------------------------------

namespace {

struct ConflictState {
    json::Value model;
    bool pending = false;
    bool answered = false;
    std::vector<std::string> answers;  // per row, whatever its widgets show
    std::vector<std::pair<size_t, RadioGroup*>> shown;
};

int radio_of(const std::string& word, bool allow_replace) {
    if (word == "skip") return 0;
    if (word == "keep-both") return allow_replace ? 2 : 1;
    return allow_replace ? 1 : 0;
}

}  // namespace

std::unique_ptr<Widget> conflicts_dialog(DialogContext& cx) {
    MainView& main = cx.main;
    const json::Value q_ = query(main.core(), "conflicts");
    auto st = std::make_shared<ConflictState>();
    st->pending = !q_["pending"].is_null();
    st->model = st->pending ? q_["pending"] : q_["last"];
    const json::Value& m = st->model;
    if (m.is_null()) {
        Card c = make_card(L"Items already exist", 620);
        f::note(*c.body, L"No transfer is waiting on an answer.");
        add_button(c, L"Close", cx.close, true);
        return std::move(c.root);
    }
    Card c = make_card(w(m["title"]), 620);
    c.body->name = "ConflictBody";
    f::text(*c.body, w(m["summary"]), tok::TextPrimary, 14.0f)->wrap = true;
    f::text(*c.body, w(m["subtitle"]), tok::TextSecondary, 13.0f)->wrap = true;

    const json::Value& rows = m["rows"];
    const bool allow_replace_any = m["allowReplace"].boolean();
    for (size_t i = 0; i < rows.size(); ++i) st->answers.push_back(rows[i]["default"].str());

    auto* scroll = c.body->emplace<ScrollViewer>();
    scroll->name = "ConflictRows";
    scroll->height = 340;
    scroll->style.bg = tok::InputBackground;
    scroll->style.border = tok::BorderSubtle;
    auto* col = scroll->emplace<StackPanel>(Orientation::Vertical, 0.0f);
    const size_t shown = std::min<size_t>(rows.size(), 300);
    for (size_t i = 0; i < shown; ++i) {
        const json::Value& r = rows[i];
        auto* item = col->emplace<StackPanel>(Orientation::Vertical, 3.0f);
        item->pad = 10;
        f::text(*item, w(r["name"]), tok::TextPrimary, 13.0f, 600);
        auto* line1 = f::row(*item, 6);
        auto* l1 = dim(*line1, L"Incoming:");
        l1->width = 84;
        auto* d1 = dim(*line1, w(r["incoming"]), r["incomingIsNewer"].boolean() ? tok::TextLink : tok::TextSecondary);
        if (r["incomingIsNewer"].boolean()) d1->style.font.weight = 600;
        d1->flex = 1;
        auto* line2 = f::row(*item, 6);
        dim(*line2, w(r["existingLabel"]))->width = 84;
        dim(*line2, w(r["existing"]), tok::TextSecondary)->flex = 1;
        const bool allow = r["allowReplace"].boolean();
        std::vector<std::wstring> labels = {L"Skip"};
        if (allow) labels.push_back(L"Replace");
        labels.push_back(L"Keep both");
        auto* rg = f::radios(*item, labels, radio_of(st->answers[i], allow), [st, i, allow](int sel) {
            const char* words[] = {"skip", "replace", "keep-both"};
            st->answers[i] = allow ? words[sel] : (sel == 0 ? "skip" : "keep-both");
        });
        rg->orientation = Orientation::Horizontal;
        st->shown.push_back({i, rg});
        const std::wstring note = w(r["mergeNote"]);
        if (!note.empty()) dim(*item, note)->wrap = true;
    }
    if (rows.size() > shown) {
        auto* more = col->emplace<StackPanel>(Orientation::Vertical, 0.0f);
        more->pad = 10;
        dim(*more, L"…and " + std::to_wstring(rows.size() - shown) + L" more, which keep the answer shown by Apply to all.", tok::TextMuted)->wrap = true;
    }

    // One decision for forty taken names is the usual case.
    auto* all = f::row(*c.body, 8);
    dim(*all, L"Apply to all:");
    auto apply = [st, rows](const char* word) {
        for (size_t i = 0; i < st->answers.size(); ++i) {
            if (std::string(word) == "replace" && !rows[i]["allowReplace"].boolean()) continue;
            st->answers[i] = word;
        }
        for (auto& [i, rg] : st->shown) rg->set_selected(radio_of(st->answers[i], rows[i]["allowReplace"].boolean()));
    };
    f::button(*all, L"Skip", [apply] { apply("skip"); }, 70)->name = "ConflictAllSkip";
    if (allow_replace_any) f::button(*all, L"Replace", [apply] { apply("replace"); }, 80)->name = "ConflictAllReplace";
    f::button(*all, L"Keep both", [apply] { apply("keep-both"); }, 90)->name = "ConflictAllKeepBoth";
    if (allow_replace_any) f::note(*c.body, w(m["replaceHint"]));

    core::Core* core = &main.core();
    c.root->on_destroy = [st, core] {
        if (st->pending && !st->answered) {
            try {
                run(*core, "conflict-answer", "{\"cancel\":true}");
            } catch (const std::exception&) {
            }
        }
    };
    add_button(c, L"Cancel", [st, core, close = cx.close] {
        if (st->pending && !st->answered) {
            st->answered = true;
            run(*core, "conflict-answer", "{\"cancel\":true}");
        }
        close();
    });
    add_button(c, L"Continue", [st, rows, core, close = cx.close] {
        if (st->pending && !st->answered) {
            st->answered = true;
            std::string json = "{\"answers\":{";
            for (size_t i = 0; i < rows.size(); ++i) {
                json += (i ? "," : "") + q(core::wide(rows[i]["groupKey"].str())) + ":\"" + st->answers[i] + "\"";
            }
            run(*core, "conflict-answer", json + "}}");
        }
        close();
    }, true)->name = "ConflictContinue";
    return std::move(c.root);
}

// ---- administrator rights ---------------------------------------------------------------------------------

std::unique_ptr<Widget> elevation_dialog(DialogContext& cx) {
    MainView& main = cx.main;
    const json::Value q_ = query(main.core(), "elevation");
    const bool pending = !q_["pending"].is_null();
    json::Value offer = pending ? q_["pending"] : q_["last"];
    Card c = make_card(L"Administrator permission", 520);
    // The headline sits beside the shield, at a width the card can promise it; the rest wraps below.
    auto* head = f::row(*c.body, 12);
    auto* shield = head->emplace<Icon>(icons::find("Shield"), 22.0f);
    shield->style.fg = tok::TextLink;
    std::vector<std::wstring> items;
    const std::wstring headline = offer.is_null() ? L"Windows will not let this program change these items." : w(offer["headline"]);
    auto* title = f::text(*head, headline, tok::TextPrimary, 14.0f, 600);
    title->wrap = true;
    title->width = 440;
    auto* col = c.body;
    if (!offer.is_null()) {
        f::note(*col, w(offer["explanation"]));
        for (size_t i = 0; i < offer["items"].size(); ++i) items.push_back(core::wide(offer["items"][i].str()));
    }
    auto* box = col->emplace<StackPanel>(Orientation::Vertical, 2.0f);
    box->pad = 8;
    box->style.bg = tok::WindowBackground;
    box->style.border = tok::BorderSubtle;
    constexpr size_t kMaxListed = 8;
    for (size_t i = 0; i < items.size() && i < kMaxListed; ++i) f::text(*box, describe_path(items[i]), tok::TextPrimary, 13.0f);
    if (items.size() > kMaxListed) dim(*box, L"and " + std::to_wstring(items.size() - kMaxListed) + L" more");
    core::Core* core = &main.core();
    auto answered = std::make_shared<bool>(false);
    c.root->on_destroy = [pending, answered, core] {
        if (pending && !*answered) {
            try {
                run(*core, "elevation-answer", "no");
            } catch (const std::exception&) {
            }
        }
    };
    add_button(c, L"Skip", [pending, answered, core, close = cx.close] {
        if (pending && !*answered) {
            *answered = true;
            run(*core, "elevation-answer", "no");
        }
        close();
    })->name = "ElevationSkip";
    add_button(c, L"Try as administrator", [pending, answered, core, close = cx.close] {
        if (pending && !*answered) {
            *answered = true;
            run(*core, "elevation-answer", "yes");
        }
        close();
    }, true, 160)->name = "ElevationContinue";
    return std::move(c.root);
}

// ---- the delete confirmation ------------------------------------------------------------------------------

// The "are you sure" before a delete: every item and where it lives, what the rules refused, and - as the
// survey walks the trees off the UI thread - how much each item amounts to. What is listed is what goes.
// Inside an archive it is the one question the rewrite needs. Spec: Views/DeleteDialog,
// DirectoryTabView.DeleteSelectionAsync, MainWindow.DeleteTreeFolderAsync.
std::unique_ptr<Widget> delete_dialog(DialogContext& cx, bool permanent) {
    MainView& main = cx.main;
    core::Core* core = &main.core();
    std::string arg = std::string("{\"permanent\":") + (permanent ? "true" : "false");
    if (!main.dialog_paths().empty()) arg += ",\"paths\":" + qa(main.dialog_paths());
    else if (!main.dialog_folder().empty()) arg += ",\"paths\":" + qa({main.dialog_folder()});
    // Whoever asked for this one (the duplicate finder) hears how it ended.
    auto after = std::make_shared<std::function<void(bool)>>(main.take_after_delete());
    arg += "}";
    json::Value p;
    std::wstring failed;
    try {
        p = query(*core, "delete-preview", arg);
    } catch (const std::exception& e) {
        failed = core::wide(e.what());
    }
    const bool has_work = p["hasWork"].boolean();
    Card c = make_card(p.is_null() || p["title"].is_null() ? L"Delete" : w(p["title"]), 560);
    if (!has_work) {
        // Nothing survived the rules: say why rather than appearing to do nothing.
        f::text(*c.body, failed.empty() ? w(p["problems"]) : failed, tok::TextPrimary, 13.5f)->wrap = true;
        add_button(c, L"OK", cx.close, true)->name = "DeleteOk";
        c.root->on_destroy = [after] {
            if (*after) (*after)(false);
        };
        return std::move(c.root);
    }
    // Inside an archive there is only one kind of delete, whichever key asked for it.
    const bool erase = p["permanent"].boolean();
    auto* head = f::row(*c.body, 12);
    auto* glyph = head->emplace<Icon>(icons::find(erase ? "Error" : "Warning"), 22.0f);
    glyph->style.fg = erase ? tok::ErrorForeground : tok::TextLink;
    auto* heading = f::text(*head, w(p["heading"]), tok::TextPrimary, 14.0f, 600);
    heading->wrap = true;
    heading->width = 480;
    heading->name = "DeleteHeading";
    if (erase) {
        f::text(*c.body, L"This cannot be undone. These items are erased, not set aside — nothing will bring them back.",
                tok::ErrorForeground, 13.0f)
            ->wrap = true;
    }
    if (!p["problems"].is_null()) {
        auto* problems = f::text(*c.body, w(p["problems"]), tok::ErrorForeground, 13.0f);
        problems->wrap = true;
        problems->name = "DeleteProblems";
    }
    const json::Value items = p["items"];
    // Usually every item is in one folder: say it once, and give the right-hand column to the sizes.
    bool one_place = items.size() > 0;
    for (size_t i = 1; i < items.size() && one_place; ++i) one_place = items[i]["location"].str() == items[0]["location"].str();
    if (one_place) dim(*c.body, L"In " + w(items[0]["location"]))->name = "DeleteLocation";
    f::ItemList* list = nullptr;
    if (items.size() > 0) {
        list = c.body->emplace<f::ItemList>();
        list->name = "DeleteItems";
        list->height = std::min<float>(240.0f, 8.0f + 40.0f * static_cast<float>(items.size()));
    }
    auto fill_list = [list, items, one_place](const json::Value& details) {
        if (!list) return;
        std::vector<f::ListEntry> entries;
        for (size_t i = 0; i < items.size(); ++i) {
            f::ListEntry e;
            e.text = w(items[i]["name"]);
            const std::wstring d = i < details.size() ? w(details[i]) : std::wstring();
            e.detail = one_place ? d : w(items[i]["location"]) + (d.empty() ? L"" : L"   ·   " + d);
            entries.push_back(std::move(e));
        }
        list->set_entries(std::move(entries));
    };
    fill_list(json::Value{});
    if (!p["archive"].boolean()) {
        auto* totals = c.body->emplace<LiveBody>(main, &MainView::on_delete_survey, [core, fill_list](LiveBody& body) {
            json::Value s;
            try {
                s = query(*core, "delete-survey");
            } catch (const std::exception&) {
            }
            dim(body, w(s["totals"]))->name = "DeleteTotals";
            fill_list(s["details"]);
        });
        (void)totals;
    }
    if (!p["undoHint"].str().empty()) f::note(*c.body, w(p["undoHint"]));
    // Closing it any way but Delete forgets the plan and stops the survey.
    auto confirmed = std::make_shared<bool>(false);
    c.root->on_destroy = [core, confirmed, after] {
        if (!*confirmed) {
            try {
                run(*core, "delete-cancel");
            } catch (const std::exception&) {
            }
        }
        if (*after) (*after)(*confirmed);
    };
    add_button(c, L"Cancel", cx.close)->name = "DeleteCancel";
    add_button(c, p["archive"].boolean() ? L"Remove" : L"Delete", [core, confirmed, after, &main, close = cx.close] {
        *confirmed = true;
        try {
            run(*core, "delete-confirm");
        } catch (const std::exception& e) {
            bb_status_set(core->app(), core::s(core::wide(e.what())));
        }
        close();
        if (!*after) main.focus_file_list();
    }, true)->name = "DeleteConfirm";
    return std::move(c.root);
}

// The one Clear of the undo history - the History window's button, the Settings page's and the command all
// ask this first, because nothing in it can be undone or redone afterwards. Spec: UndoHistoryWindow.ConfirmAndClear.
std::unique_ptr<Widget> clear_undo_dialog(DialogContext& cx) {
    core::Core* core = &cx.main.core();
    const json::Value h = query(*core, "history");
    Card c = make_card(L"Clear undo history", 480);
    f::text(*c.body, w(h["clearQuestion"]), tok::TextPrimary, 13.5f)->wrap = true;
    add_button(c, L"Cancel", cx.close)->name = "ClearUndoCancel";
    add_button(c, L"Clear history", [core, close = cx.close] {
        bb_undo_clear(core->app());
        close();
    }, true, 120)->name = "ClearUndoConfirm";
    return std::move(c.root);
}

// ---- a yes/no question, and something to be told ------------------------------------------------------------

// What a worker asked through the confirm seam (a flat view of a huge folder, an update that is ready). The
// worker is parked until this answers; closing the card any other way is "no". Spec: Services/UserConfirm.
std::unique_ptr<Widget> confirm_dialog(DialogContext& cx) {
    core::Core* core = &cx.main.core();
    const json::Value q = query(*core, "confirm")["pending"];
    const bool pending = !q.is_null();
    Card c = make_card(pending ? w(q["title"]) : L"Confirm", 480);
    f::text(*c.body, pending ? w(q["message"]) : L"", tok::TextPrimary, 13.5f)->wrap = true;
    auto answered = std::make_shared<bool>(false);
    auto answer = [pending, answered, core](bool yes) {
        if (!pending || *answered) return;
        *answered = true;
        try {
            run(*core, "confirm-answer", yes ? "yes" : "no");
        } catch (const std::exception&) {
        }
    };
    c.root->on_destroy = [answer] { answer(false); };
    add_button(c, pending ? w(q["confirm"]).c_str() : L"OK", [answer, close = cx.close] {
        answer(true);
        close();
    }, true, 140)->name = "ConfirmYes";
    add_button(c, L"Cancel", [answer, close = cx.close] {
        answer(false);
        close();
    })->name = "ConfirmNo";
    return std::move(c.root);
}

// A message the user has to read (a comparison that was refused, and why). One at a time, oldest first.
std::unique_ptr<Widget> notice_dialog(DialogContext& cx) {
    core::Core* core = &cx.main.core();
    const json::Value q = query(*core, "notice")["pending"];
    Card c = make_card(q.is_null() ? L"BertBrowser" : w(q["title"]), 460);
    f::text(*c.body, q.is_null() ? L"" : w(q["message"]), tok::TextPrimary, 13.5f)->wrap = true;
    if (!q.is_null()) {
        c.root->on_destroy = [core] {
            try {
                run(*core, "notice-dismiss");
            } catch (const std::exception&) {
            }
        };
    }
    add_button(c, L"OK", cx.close, true)->name = "NoticeOk";
    return std::move(c.root);
}

// ---- the History window ---------------------------------------------------------------------------------

std::unique_ptr<Widget> history_dialog(DialogContext& cx) {
    MainView& main = cx.main;
    auto expanded = std::make_shared<std::set<int64_t>>();
    std::shared_ptr<std::wstring> footer = std::make_shared<std::wstring>();
    auto fill = [&main, expanded](LiveBody& body) {
        core::Core* core = &main.core();
        const json::Value h = query(*core, "history");
        const json::Value& rows = h["rows"];
        {
            bool can_undo = false, can_redo = false;
            for (size_t i = 0; i < rows.size(); ++i) (rows[i]["isDone"].boolean() ? can_undo : can_redo) = true;
            auto* head = f::row(body, 8);
            auto* undo = f::button(*head, L"Undo", [core] {
                uint64_t op = 0;
                bb_undo_step(core->app(), 1, &op);
            }, 90);
            undo->name = "HistoryUndo";
            undo->set_enabled(can_undo && h["canStep"].boolean());
            auto* redo = f::button(*head, L"Redo", [core] {
                uint64_t op = 0;
                bb_undo_step(core->app(), -1, &op);
            }, 90);
            redo->name = "HistoryRedo";
            redo->set_enabled(can_redo && h["canStep"].boolean());
        }
        if (rows.size() == 0) {
            f::note(body, L"Nothing has been done yet that can be taken back. Moves, renames, deletes and the like appear here.");
        } else {
            auto* scroll = body.emplace<ScrollViewer>();
            scroll->name = "HistoryRows";
            scroll->height = 380;
            scroll->style.bg = tok::InputBackground;
            scroll->style.border = tok::BorderSubtle;
            auto* col = scroll->emplace<StackPanel>(Orientation::Vertical, 0.0f);
            // Undone entries first (newest on top), then the cursor, then what is in effect.
            bool any_undone = false;
            for (size_t i = 0; i < rows.size(); ++i) any_undone = any_undone || !rows[i]["isDone"].boolean();
            auto add_row = [&](const json::Value& r) {
                const bool done = r["isDone"].boolean();
                const int64_t id = r["id"].i64();
                auto* item = col->emplace<StackPanel>(Orientation::Vertical, 2.0f);
                item->pad = 8;
                item->name = "HistoryRow" + std::to_string(id);
                auto* line = f::row(*item, 8);
                if (const IconData* ic = icon_for(r["icon"].str())) {
                    auto* icon = line->emplace<Icon>(ic, 16.0f);
                    icon->style.fg = done ? tok::TextPrimary : tok::TextMuted;
                }
                auto* title = f::text(*line, w(r["description"]), done ? tok::TextPrimary : tok::TextMuted, 13.0f, 600);
                title->flex = 1;
                const std::wstring badge = w(r["badge"]);
                if (!badge.empty()) dim(*line, badge, r["problem"].str().empty() ? tok::TextSecondary : tok::WarningForeground);
                const int steps = static_cast<int>(r["steps"].i64());
                const bool can_step = h["canStep"].boolean();
                auto* go = f::button(*line, done ? L"Undo to here" : L"Redo to here", [core, steps, done] {
                    uint64_t op = 0;
                    bb_undo_step(core->app(), done ? steps : -steps, &op);
                }, 110);
                go->set_enabled(can_step);
                go->name = (done ? "UndoTo" : "RedoTo") + std::to_string(id);
                std::wstring meta = w(r["whenText"]) + L"  ·  " + w(r["itemsText"]);
                const std::wstring held = w(r["heldText"]);
                if (!held.empty()) meta += L"  " + held;
                const bool open = expanded->count(id) > 0;
                auto* sub = f::row(*item, 8);
                dim(*sub, meta, tok::TextMuted)->flex = 1;
                const std::wstring reveal = w(r["revealPath"]);
                if (!reveal.empty()) {
                    // A folder opens, a file is shown where it is, and one that has gone shows its folder.
                    // Spec: UndoHistoryWindow.Reveal_Click.
                    f::button(*sub, L"Show", [&main, reveal] {
                        std::wstring path = reveal;
                        DWORD attrs = GetFileAttributesW(path.c_str());
                        if (attrs == INVALID_FILE_ATTRIBUTES) {
                            path = path.substr(0, path.find_last_of(L"\\/"));
                            attrs = GetFileAttributesW(path.c_str());
                            if (attrs == INVALID_FILE_ATTRIBUTES) return;
                        }
                        const bool dir = (attrs & FILE_ATTRIBUTE_DIRECTORY) != 0;
                        try_tool_call(main, "reveal", "{\"path\":" + json_quote(path) + ",\"isDirectory\":" + (dir ? "true" : "false") + "}");
                    }, 70)->name = "HistoryShow" + std::to_string(id);
                }
                f::button(*sub, open ? L"Hide items" : L"Items", [expanded, id, &main] {
                    if (!expanded->erase(id)) expanded->insert(id);
                    if (main.on_undo_changed) main.on_undo_changed();
                }, 90);
                const std::wstring problem = w(r["problem"]);
                if (!problem.empty()) dim(*item, problem, tok::WarningForeground)->wrap = true;
                if (open) {
                    for (size_t d = 0; d < r["details"].size(); ++d) dim(*item, core::wide(r["details"][d].str()), tok::TextSecondary);
                }
            };
            for (size_t i = 0; i < rows.size(); ++i) {
                if (!rows[i]["isDone"].boolean()) add_row(rows[i]);
            }
            if (any_undone) {
                auto* marker = f::note(*col, L"Undone above — redo steps forward · In effect below — undo steps back");
                marker->name = "HistoryCursor";
            }
            for (size_t i = 0; i < rows.size(); ++i) {
                if (rows[i]["isDone"].boolean()) add_row(rows[i]);
            }
        }
        dim(body, w(h["footer"]))->name = "HistoryFooter";
        const std::wstring released = w(h["releasedText"]);
        if (!released.empty()) dim(body, released, tok::WarningForeground)->name = "HistoryReleased";
    };
    auto body = std::make_unique<LiveBody>(main, &MainView::on_undo_changed, fill);
    Card c = make_card(L"History", 640, std::move(body));
    core::Core* core = &main.core();
    add_button(c, L"History settings…", [&main, close = cx.close] {
        close();
        bb_settings_page_open(main.core().app(), core::s(L"history"));
        main.sync_overlays();
    }, false, 150);
    add_button(c, L"Clear history", [&main] { main.open_dialog(L"clear-undo"); }, false, 120)
        ->set_enabled(query(*core, "history")["count"].i64() > 0);
    add_button(c, L"Close", cx.close, true);
    return std::move(c.root);
}

// ---- the table ---------------------------------------------------------------------------------------------

void add_dialogs(std::map<std::wstring, DialogBuilder>& table) {
    table[L"transfer"] = [](DialogContext& cx) { return transfer_dialog(cx); };
    table[L"conflicts"] = [](DialogContext& cx) { return conflicts_dialog(cx); };
    table[L"elevation"] = [](DialogContext& cx) { return elevation_dialog(cx); };
    table[L"confirm"] = [](DialogContext& cx) { return confirm_dialog(cx); };
    table[L"clear-undo"] = [](DialogContext& cx) { return clear_undo_dialog(cx); };
    table[L"delete"] = [](DialogContext& cx) { return delete_dialog(cx, false); };
    table[L"delete-permanently"] = [](DialogContext& cx) { return delete_dialog(cx, true); };
    table[L"notice"] = [](DialogContext& cx) { return notice_dialog(cx); };
    table[L"undo-history"] = [](DialogContext& cx) { return history_dialog(cx); };
    table[L"rename"] = [](DialogContext& cx) { return rename_dialog(cx, false); };
    table[L"rename-advanced"] = [](DialogContext& cx) { return rename_dialog(cx, true); };
    table[L"compress"] = [](DialogContext& cx) { return compress_dialog(cx); };
    table[L"extract"] = [](DialogContext& cx) { return extract_dialog(cx); };
    table[L"archive-password"] = [](DialogContext& cx) { return password_dialog(cx); };
}

}  // namespace bb::views::ops
