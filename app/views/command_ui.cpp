#include "views/command_ui.h"

#include "gfx/icons.gen.h"

#include <algorithm>
#include <cmath>

namespace bb::views {

using namespace bb::ui;

// ---- rows ---------------------------------------------------------------------------------------------

BarRow read_bar_row(const json::Value& v) {
    BarRow r;
    r.kind = v["kind"].str();
    r.id = v["id"].str();
    r.icon = v["icon"].str();
    r.caption = core::wide(v["caption"].str());
    r.name = core::wide(v["name"].str());
    r.gesture = core::wide(v["gesture"].str());
    r.reason = core::wide(v["reason"].str());
    r.checked = v["checked"].boolean();
    r.enabled = v["enabled"].boolean(true);
    r.show_text = v["showText"].boolean();
    r.children = read_bar_rows(v["children"]);
    return r;
}

std::vector<BarRow> read_bar_rows(const json::Value& rows) {
    std::vector<BarRow> out;
    for (size_t i = 0; i < rows.size(); ++i) out.push_back(read_bar_row(rows[i]));
    return out;
}

std::string bar_token(const BarRow& r) {
    if (r.kind == "separator") return "-";
    if (r.kind == "list" || r.kind == "placeholder") return "@" + r.id;
    if (r.kind == "menu") return "menu:" + core::utf8(r.caption);
    return r.id;
}

const ui::IconData* icon_for(const std::string& name) {
    if (name.empty()) return nullptr;
    const std::string bare = name.rfind("Icon.", 0) == 0 ? name.substr(5) : name;
    return icons::find(bare.c_str());
}

std::wstring plain_caption(const std::wstring& caption) { return parse_menu_label(caption).display; }

std::wstring escape_caption(const std::wstring& text) {
    std::wstring out;
    for (wchar_t c : text) {
        if (c == L'&') out += L'&';
        out += c;
    }
    return out;
}

// ---- menus --------------------------------------------------------------------------------------------

static void append_rows(const MenuBuildEnv& env, const std::vector<BarRow>& rows, MenuItems& out) {
    for (const BarRow& r : rows) {
        if (r.kind == "separator") {
            out.push_back(MenuItem::separator());
        } else if (r.kind == "menu") {
            MenuItem m = MenuItem::make(r.caption);
            m.icon = icon_for(r.icon);
            append_rows(env, r.children, m.sub);
            if (!m.sub.empty()) out.push_back(std::move(m));
        } else if (r.kind == "list") {
            const json::Value items = try_model([&](BbOwnedStr* o) {
                return bb_bars_query(env.core->app(), core::s(L"list"), core::s(core::wide(r.id)), o);
            });
            for (size_t i = 0; i < items.size(); ++i) {
                // An undo step is a number; everything else is named.
                const json::Value& raw = items[i]["id"];
                const std::wstring id = raw.type == json::Value::Type::Number ? std::to_wstring(raw.i64()) : core::wide(raw.str());
                const std::wstring name = core::wide(items[i]["name"].str());
                // The detail (a workspace's shape, a saved search's query) rides where a gesture would.
                MenuItem m = MenuItem::make(escape_caption(name), {}, core::wide(items[i]["detail"].str()));
                m.icon = icon_for(r.icon);
                m.checked = items[i]["checked"].boolean();
                m.enabled = items[i]["enabled"].boolean(true);
                m.tooltip = core::wide(items[i]["tip"].str());
                m.on_invoke = [env, list = r.id, id, name] {
                    if (env.pick) env.pick(list, id, name);
                };
                out.push_back(std::move(m));
            }
        } else if (r.kind == "placeholder") {
            MenuItem m = MenuItem::make(escape_caption(r.name.empty() ? r.caption : r.name));
            m.enabled = false;
            out.push_back(std::move(m));
        } else {
            MenuItem m = MenuItem::make(r.caption, {}, r.gesture);
            m.icon = icon_for(r.icon);
            m.enabled = r.enabled;
            m.checked = r.checked;
            if (!r.enabled) m.tooltip = r.reason;
            m.on_invoke = [env, id = core::wide(r.id)] {
                if (env.run) env.run(id);
            };
            out.push_back(std::move(m));
        }
    }
}

MenuItems build_menu_items(const MenuBuildEnv& env, const std::vector<BarRow>& rows) {
    MenuItems out;
    append_rows(env, rows, out);
    return out;
}

// ---- the toolbar --------------------------------------------------------------------------------------

namespace {
constexpr float kBtn = 32.0f, kSep = 9.0f, kPadX = 4.0f, kOverflowW = 28.0f, kIcon = 16.0f;
}

// Whether a refresh finds a row exactly as it was, children and all.
static bool same_row(const BarRow& a, const BarRow& b) {
    if (a.kind != b.kind || a.id != b.id || a.icon != b.icon || a.caption != b.caption || a.name != b.name || a.gesture != b.gesture ||
        a.reason != b.reason || a.checked != b.checked || a.enabled != b.enabled || a.show_text != b.show_text || a.hidden != b.hidden ||
        a.children.size() != b.children.size())
        return false;
    for (size_t i = 0; i < a.children.size(); ++i) {
        if (!same_row(a.children[i], b.children[i])) return false;
    }
    return true;
}

CommandToolbar::CommandToolbar(std::string which) : which_(std::move(which)) {
    height = kHeight;
    flags.focusable = false;  // a button that took the focus would run its command against a list that just lost it
    style.bg = tok::ToolbarBackground;
}

float CommandToolbar::item_width(const BarRow& r) const {
    if (is_split(r)) {
        BarRow face = r;
        face.id.clear();
        return item_width(face) + kChevron;
    }
    if (r.kind == "separator") return kSep;
    if (r.kind == "list" || r.kind == "placeholder") return 40.0f;
    if (r.kind == "menu") {
        // A submenu has no picture, so it is its caption and the chevron that says it drops down.
        // Spec: CommandBarToolbar.UpdateButton.
        const float t = std::ceil(res().text.measure(plain_caption(r.caption), gfx::FontSpec{}).width);
        return 8.0f + t + 5.0f + 10.0f + 8.0f;
    }
    if (r.show_text) {
        const float t = std::ceil(res().text.measure(plain_caption(r.caption), gfx::FontSpec{}).width);
        return 8.0f + (icon_for(r.icon) ? kIcon + 6.0f : 0.0f) + t + 8.0f;
    }
    return kBtn;
}

void CommandToolbar::update(const json::Value& model) {
    const json::Value& rows = model["rows"];
    std::string sig = model["showText"].boolean() ? "T" : "t";
    for (size_t i = 0; i < rows.size(); ++i) {
        sig += rows[i]["kind"].str() + ":" + rows[i]["id"].str() + ":" + (rows[i]["showText"].boolean() ? "T" : "t") + ";";
    }
    show_text_ = model["showText"].boolean();
    if (sig == signature_ && rows.size() == items_.size()) {
        // The whole row is taken, children included, so a dropdown opened from it is as fresh as the button.
        bool changed = false, refit = false;
        for (size_t i = 0; i < rows.size(); ++i) {
            BarRow fresh = read_bar_row(rows[i]);
            BarRow& old = items_[i].row;
            if (!same_row(old, fresh)) {
                // A caption shown beside the icon changes the button's width. Spec: CommandBarToolbar.Show.
                if ((fresh.show_text || fresh.kind == "menu") && fresh.caption != old.caption) refit = true;
                old = std::move(fresh);
                changed = true;
            }
        }
        if (refit) {
            laid_width_ = -1;
            request_layout();
        }
        if (changed) invalidate();
        return;
    }
    signature_ = sig;
    items_.clear();
    for (size_t i = 0; i < rows.size(); ++i) items_.push_back({read_bar_row(rows[i]), 0, 0});
    icon_geo_.assign(items_.size(), {});
    icon_gen_.assign(items_.size(), 0);
    laid_width_ = -1;
    request_layout();
    invalidate();
}

void CommandToolbar::lay_out(float avail) {
    laid_width_ = avail;
    float total = kPadX;
    for (Item& it : items_) {
        it.w = item_width(it.row);
        total += it.w;
    }
    const bool all = total + kPadX <= avail;
    const float limit = all ? avail : avail - kOverflowW - kPadX;
    float x = kPadX;
    visible_ = 0;
    for (Item& it : items_) {
        if (x + it.w > limit && !all) break;
        it.x = x;
        x += it.w;
        ++visible_;
    }
    while (!all && visible_ > 0 && items_[visible_ - 1].row.kind == "separator") --visible_;
}

Size CommandToolbar::measure(Size) {
    float total = 2 * kPadX;
    for (const Item& it : items_) total += item_width(it.row);
    return {width >= 0 ? width : total, kHeight};
}

void CommandToolbar::arrange(Rect r) {
    bounds = r;
    lay_out(r.width());
}

Rect CommandToolbar::item_rect(size_t i) const {
    const Item& it = items_[i];
    return Rect{bounds.l + it.x, bounds.t + 2, bounds.l + it.x + it.w, bounds.b - 2};
}

Rect CommandToolbar::chevron_rect(size_t i) const {
    const Rect rc = item_rect(i);
    return Rect{rc.r - kChevron, rc.t, rc.r, rc.b};
}

Rect CommandToolbar::overflow_rect() const {
    const float x = visible_ == 0 ? kPadX : items_[visible_ - 1].x + items_[visible_ - 1].w;
    return Rect{bounds.l + x, bounds.t + 2, bounds.l + x + kOverflowW, bounds.b - 2};
}

int CommandToolbar::index_at(Point p) const {
    for (size_t i = 0; i < visible_ && i < items_.size(); ++i) {
        if (items_[i].row.kind != "separator" && item_rect(i).contains(p)) return static_cast<int>(i);
    }
    if (visible_ < items_.size() && overflow_rect().contains(p)) return kOverflow;
    return -1;
}

Widget* CommandToolbar::hit_test(Point p) {
    if (!flags.visible || !bounds.contains(p)) return nullptr;
    return index_at(p) != -1 ? this : nullptr;
}

void CommandToolbar::paint(PaintContext& pc) {
    gfx::FontSpec font;
    for (size_t i = 0; i < visible_ && i < items_.size(); ++i) {
        Item& it = items_[i];
        const BarRow& r = it.row;
        const Rect rc = item_rect(i);
        if (r.kind == "separator") {
            const float x = std::floor((rc.l + rc.r) / 2) + 0.5f;
            pc.line({x, rc.t + 5}, {x, rc.b - 5}, tok::BorderSubtle);
            continue;
        }
        const bool hot = hot_ == static_cast<int>(i), down = pressed_ == static_cast<int>(i);
        const bool split = is_split(r);
        const Rect face = split ? Rect{rc.l, rc.t, rc.r - kChevron, rc.b} : rc;
        if (split && r.enabled && (hot || down)) {
            // The half under the pointer is the one lit, so it is plain which of the two a click is.
            const bool on_chevron = down ? pressed_chevron_ : chevron_rect(i).contains(root() ? root()->pointer() : Point{});
            pc.fill(on_chevron ? chevron_rect(i) : face, down ? tok::ToolbarPressedBackground : tok::ToolbarHoverBackground);
        } else if (r.enabled && (down || r.checked)) {
            pc.fill(rc, tok::ToolbarPressedBackground);
        } else if (r.enabled && hot) {
            pc.fill(rc, tok::ToolbarHoverBackground);
        }
        const Token fg = !r.enabled ? tok::TextDisabled : r.checked ? tok::ToolbarCheckedForeground : tok::ToolbarForeground;
        if (split) {
            gfx::FontSpec small_font;
            small_font.size = 9.0f;
            pc.text(L"\u25BE", chevron_rect(i), small_font, fg, gfx::TextAlign::Center);
        }
        const IconData* icon = icon_for(r.icon);
        float x = face.l + (r.show_text ? 8.0f : (face.width() - kIcon) / 2);
        if (icon) {
            Icon::draw(pc, *icon, Rect::xywh(x, rc.t + (rc.height() - kIcon) / 2, kIcon, kIcon), fg, icon_geo_[i], icon_gen_[i]);
            x += kIcon + 6.0f;
        }
        if (r.kind == "list" || r.kind == "placeholder") {
            pc.text(L"▾", Rect{x - 2, rc.t, rc.r, rc.b}, font, fg);
        } else if (r.kind == "menu") {
            pc.text(plain_caption(r.caption), Rect{x, rc.t, rc.r - 4, rc.b}, font, fg);
            pc.text(L"▾", Rect{rc.r - 8 - 10, rc.t, rc.r - 4, rc.b}, font, fg);
        } else if (r.show_text || !icon) {
            pc.text(plain_caption(r.caption), Rect{x, rc.t, rc.r - 4, rc.b}, font, fg);
        }
    }
    if (visible_ < items_.size()) {
        const Rect rc = overflow_rect();
        if (hot_ == kOverflow) pc.fill(rc, tok::ToolbarHoverBackground);
        pc.text(L"»", rc, font, tok::ToolbarForeground, gfx::TextAlign::Center);
    }
}

bool CommandToolbar::on_mouse_down(const MouseEvent& e) {
    if (e.button == MouseButton::Right && on_context) {
        const int at = index_at(e.pt);
        if (at >= 0 && at != kOverflow && items_[static_cast<size_t>(at)].row.kind == "command") {
            on_context(items_[static_cast<size_t>(at)].row, item_rect(static_cast<size_t>(at)));
            return true;
        }
        return false;
    }
    if (e.button != MouseButton::Left) return false;
    pressed_ = index_at(e.pt);
    pressed_chevron_ = pressed_ >= 0 && is_split(items_[static_cast<size_t>(pressed_)].row) &&
                       chevron_rect(static_cast<size_t>(pressed_)).contains(e.pt);
    invalidate();
    return pressed_ != -1;
}

bool CommandToolbar::on_mouse_up(const MouseEvent& e) {
    const int down = pressed_;
    pressed_ = -1;
    invalidate();
    if (down == -1 || index_at(e.pt) != down) return false;
    if (down == kOverflow) {
        // Not starting on a rule. Spec: MainWindow.CommandBars (SkipWhile Separator).
        std::vector<BarRow> rest;
        for (size_t i = visible_; i < items_.size(); ++i) {
            if (rest.empty() && items_[i].row.kind == "separator") continue;
            rest.push_back(items_[i].row);
        }
        if (on_overflow) on_overflow(rest, overflow_rect());
        return true;
    }
    const Item& it = items_[static_cast<size_t>(down)];
    if (pressed_chevron_) {
        pressed_chevron_ = false;
        if (it.row.enabled && on_context) on_context(it.row, item_rect(static_cast<size_t>(down)));
        return true;
    }
    if (it.row.enabled && on_activate) on_activate(it.row, item_rect(static_cast<size_t>(down)));
    return true;
}

bool CommandToolbar::on_mouse_move(const MouseEvent& e) {
    const int at = index_at(e.pt);
    const bool on_chevron = at >= 0 && is_split(items_[static_cast<size_t>(at)].row) && chevron_rect(static_cast<size_t>(at)).contains(e.pt);
    if (at >= 0 && at == hot_ && is_split(items_[static_cast<size_t>(at)].row)) invalidate(item_rect(static_cast<size_t>(at)));
    if (at != hot_ || on_chevron != hot_chevron_) {
        hot_ = at;
        hot_chevron_ = on_chevron;
        if (at == kOverflow) {
            tooltip = L"More";
        } else if (at >= 0) {
            // What it is and its shortcut, the live caption ("Undo move 3 items"); or, greyed, why not.
            // Spec: CommandBarToolbar.UpdateButton / Update (the chevron is "{Name} history").
            const BarRow& r = items_[static_cast<size_t>(at)].row;
            const std::wstring caption = plain_caption(r.caption).empty() ? r.name : plain_caption(r.caption);
            if (on_chevron) tooltip = (r.name.empty() ? caption : r.name) + L" history";
            else if (!r.enabled && !r.reason.empty()) tooltip = (r.name.empty() ? caption : r.name) + L" \u2014 " + r.reason;
            else if (!r.gesture.empty()) tooltip = caption + L" (" + r.gesture + L")";
            else tooltip = caption;
        } else {
            tooltip.clear();
        }
        invalidate();
    }
    return false;
}

void CommandToolbar::on_mouse_leave() {
    hot_ = -1;
    hot_chevron_ = false;
    pressed_ = -1;
    tooltip.clear();
    invalidate();
}

}  // namespace bb::views
