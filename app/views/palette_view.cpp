#include "views/palette_view.h"

#include "views/command_ui.h"
#include "views/main_view.h"

#include <windows.h>

#include <algorithm>

namespace bb::views {

using namespace bb::ui;
namespace f = forms;

namespace {
constexpr float kCardWidth = 640.0f, kRowHeight = 30.0f;
constexpr size_t kMaxRows = 12;
}  // namespace

PaletteView::PaletteView(MainView& main, core::Core& core) : main_(main), core_(core) {
    flags.visible = false;
    auto card = std::make_unique<Panel>();
    card->name = "CommandPalette";
    card->pad = 10;
    card->style.bg = tok::OverlayBackground;
    card->style.border = tok::OverlayBorder;
    card_ = add(std::move(card));
    auto* col = card_->emplace<StackPanel>(Orientation::Vertical, 6.0f);
    box_ = col->emplace<Edit>();
    box_->name = "PaletteBox";
    box_->placeholder = L"Type a command, or a folder to go to";
    box_->on_changed = [this](const std::wstring& t) {
        if (!suppress_) send(L"type", t);
    };
    hint_ = col->emplace<TextBlock>();
    hint_->name = "PaletteHint";
    hint_->style.fg = tok::TextMuted;
    hint_->style.font.size = 12.5f;
    list_ = col->emplace<f::ItemList>();
    list_->name = "PaletteRows";
    list_->row_height = kRowHeight;
    list_->style.bg = gfx::kNoToken;
    list_->style.border = gfx::kNoToken;
    list_->flags.tab_stop = false;
    list_->on_select = [this](int i) { send(L"select", std::to_wstring(i)); };
    list_->on_activate = [this](int) { pick(); };
}

void PaletteView::send(const wchar_t* verb, const std::wstring& arg) {
    BbOwnedStr o{};
    if (bb_palette_do(core_.app(), core::s(verb), core::s(arg), &o) == BB_OK) bb_str_free(o);
    sync();
}

void PaletteView::pick() {
    BbOwnedStr o{};
    if (bb_palette_do(core_.app(), core::s(L"pick"), core::s(L""), &o) != BB_OK) return;
    const json::Value r = json::parse(core::utf8(core::take(o)));
    if (r["ui"].boolean()) main_.host_command(core::wide(r["command"].str()));
    else if (!r["command"].str().empty()) main_.after_command(core::wide(r["command"].str()));
    main_.sync_overlays();
}

bool PaletteView::preview_key(const KeyEvent& e) {
    if (!open_) return false;
    switch (e.vk) {
        case VK_DOWN: send(L"down", L"1"); return true;
        case VK_UP: send(L"up", L"1"); return true;
        case VK_NEXT: send(L"down", L"8"); return true;
        case VK_PRIOR: send(L"up", L"8"); return true;
        case VK_RETURN: pick(); return true;
        case VK_TAB: send(L"complete"); return true;
        case VK_ESCAPE: send(L"close"); return true;
        default: break;
    }
    if (e.mods.alt && !e.mods.ctrl && e.vk == 'P') {
        send(L"pin");
        return true;
    }
    if (e.mods.alt && !e.mods.ctrl && e.vk == 'K') {
        send(L"shortcut");
        main_.sync_overlays();
        return true;
    }
    return false;
}

void PaletteView::sync() {
    const json::Value st = try_model([&](BbOwnedStr* o) { return bb_palette_state(core_.app(), o); });
    const bool now_open = st["open"].boolean();
    const bool was = open_;
    open_ = now_open;
    flags.visible = now_open;
    if (!now_open) {
        if (was) {
            signature_.clear();
            request_layout();
            invalidate();
            if (root() && root()->focused() == box_) root()->set_focus(nullptr);
        }
        return;
    }
    const std::wstring query = core::wide(st["query"].str());
    if (box_->text() != query) {
        suppress_ = true;
        box_->set_text(query);
        suppress_ = false;
    }
    hint_->set_text(core::wide(st["hint"].str()));
    std::vector<f::ListEntry> entries;
    int selected = -1;
    for (size_t i = 0; i < st["rows"].size(); ++i) {
        const json::Value& r = st["rows"][i];
        const std::string kind = r["kind"].str();
        f::ListEntry e;
        e.text = core::wide(r["text"].str());
        if (kind == "heading") {
            e.heading = true;
        } else if (kind == "category") {
            e.detail = core::wide(r["token"].str()) + L"  " + std::to_wstring(r["count"].i64());
            e.icon = icon_for(r["icon"].str());
        } else {
            e.icon = icon_for(r["icon"].str());
            e.enabled = r["available"].boolean(true);
            e.detail = core::wide(r["gesture"].str());
            if (r["pinned"].boolean()) e.detail = L"★ " + e.detail;
        }
        if (r["selected"].boolean()) selected = static_cast<int>(i);
        entries.push_back(std::move(e));
    }
    if (st["empty"].boolean() || entries.empty()) {
        f::ListEntry e;
        e.text = L"Nothing matches";
        e.muted = true;
        entries.insert(entries.begin(), e);
        selected = -1;
    }
    list_->height = kRowHeight * static_cast<float>(std::min(kMaxRows, entries.size())) + 2;
    list_->set_entries(std::move(entries), selected);
    if (selected >= 0) list_->scroll_into_view(selected);
    if (!was && root()) root()->set_focus(box_);
    if (!was) box_->select(box_->text().size(), box_->text().size());
    request_layout();
    invalidate();
}

void PaletteView::arrange(Rect r) {
    bounds = r;
    const float w = std::min(kCardWidth, r.width() - 40);
    card_->measure_tree({w, r.height()});
    const float h = card_->desired().h;
    card_->arrange(Rect::xywh(r.l + (r.width() - w) / 2, r.t + 60, w, h));
}

bool PaletteView::on_mouse_down(const MouseEvent& e) {
    if (!card_->bounds.contains(e.pt)) send(L"close");
    return true;
}

}  // namespace bb::views
