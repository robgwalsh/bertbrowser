#include "ui/popups/menu_bar.h"

#include <cmath>
#include <cwctype>

namespace bb::ui {

MenuBar::MenuBar() { style.fg = tok::ToolbarForeground; }

void MenuBar::set_menus(std::vector<MenuBarEntry> menus) {
    exit_mode();
    menus_ = std::move(menus);
    lefts_.clear();
    request_layout();
    invalidate();
}

Size MenuBar::measure(Size) {
    lefts_.clear();
    float x = 0;
    for (const MenuBarEntry& m : menus_) {
        lefts_.push_back(x);
        x += std::ceil(res().text.measure(parse_menu_label(m.text).display, style.font).width) + 2 * kHeadingPad;
    }
    lefts_.push_back(x);
    return fixed_or({x, 28.0f});
}

Rect MenuBar::heading_rect(int i) const {
    if (i < 0 || static_cast<size_t>(i) + 1 >= lefts_.size()) return {};
    return Rect{bounds.l + lefts_[static_cast<size_t>(i)], bounds.t, bounds.l + lefts_[static_cast<size_t>(i) + 1], bounds.b};
}

int MenuBar::heading_at(Point p) const {
    for (int i = 0; static_cast<size_t>(i) + 1 < lefts_.size(); ++i) {
        if (heading_rect(i).contains(p)) return i;
    }
    return -1;
}

void MenuBar::paint(PaintContext& pc) {
    Widget::paint(pc);
    const bool show_acc = mode_ || alt_pending_;
    for (int i = 0; static_cast<size_t>(i) + 1 < lefts_.size(); ++i) {
        const Rect r = heading_rect(i);
        if (open_ == i) {
            pc.fill(r, tok::ToolbarPressedBackground);
        } else if ((mode_ && active_ == i) || (!mode_ && hover_ == i)) {
            pc.fill(r, tok::ToolbarHoverBackground);
        }
        if (mode_ && active_ == i && open_ < 0) pc.stroke(r, tok::BorderFocus);
        const MenuLabel label = parse_menu_label(menus_[static_cast<size_t>(i)].text);
        pc.text(label.display, r.deflate(kHeadingPad, 0, kHeadingPad, 0), style.font, style.fg);
        if (show_acc && label.key_index >= 0) {
            const D2D1_SIZE_F full = pc.res.text.measure(label.display, style.font);
            const float x0 = r.l + kHeadingPad +
                             pc.res.text.measure(label.display.substr(0, static_cast<size_t>(label.key_index)), style.font).width;
            const float x1 = r.l + kHeadingPad +
                             pc.res.text.measure(label.display.substr(0, static_cast<size_t>(label.key_index) + 1), style.font).width;
            const float y = std::floor(r.t + (r.height() - full.height) / 2 + full.height - 2.0f) + 0.5f;
            pc.line({x0, y}, {x1, y}, style.fg);
        }
    }
}

// ---- opening and modes ------------------------------------------------------------------------------

void MenuBar::open(int index, bool select_first, bool button_down) {
    if (index < 0 || static_cast<size_t>(index) >= menus_.size()) return;
    PopupLayer* layer = PopupLayer::of(*this);
    if (!layer) return;
    MenuBarEntry& entry = menus_[static_cast<size_t>(index)];
    if (entry.on_opening) entry.on_opening(entry.items);
    const bool was_mode = mode_;
    mode_ = true;
    alt_pending_ = false;
    active_ = index;
    MenuOpen o;
    o.anchor = heading_rect(index);
    o.select_first = select_first;
    o.button_down = button_down;
    o.on_closed = [this](MenuClose why) { on_dropdown_closed(why); };
    o.on_outside_press = [this](Point p) {
        const int h = heading_at(p);
        if (h < 0) return false;
        if (h == open_) {
            exit_mode();  // a second press on the open heading closes it
        } else {
            open(h, false, false);
        }
        return true;
    };
    o.on_outside_move = [this](Point p) {
        const int h = heading_at(p);
        if (h >= 0 && h != open_) open(h, false, false);
    };
    o.on_edge = [this](bool right) { step(right ? 1 : -1, true); };
    switching_ = true;  // opening replaces a dropdown that was up; that close is not an exit
    open_ = index;
    layer->open_menu(entry.items, std::move(o));
    switching_ = false;
    if (!was_mode && on_mode_changed) on_mode_changed(true);
    invalidate();
}

void MenuBar::step(int dir, bool reopen) {
    if (menus_.empty()) return;
    const int n = static_cast<int>(menus_.size());
    active_ = ((active_ < 0 ? 0 : active_) + dir + n) % n;
    if (reopen) {
        open(active_, true);
    } else {
        invalidate();
    }
}

void MenuBar::on_dropdown_closed(MenuClose why) {
    if (switching_) return;
    open_ = -1;
    if (why == MenuClose::Escape && mode_ && root()) {
        root()->set_modal(this);  // Esc in a dropdown steps back to the bar, still in mode
        invalidate();
    } else {
        exit_mode();
    }
}

void MenuBar::enter_mode() {
    if (mode_ || menus_.empty() || !root() || !allowed()) return;
    mode_ = true;
    active_ = 0;
    root()->set_modal(this);
    invalidate();
    if (on_mode_changed) on_mode_changed(true);
}

void MenuBar::exit_mode() {
    if (!mode_) return;
    mode_ = false;
    alt_pending_ = false;
    active_ = -1;
    if (open_ >= 0) {
        PopupLayer* layer = PopupLayer::of(*this);
        open_ = -1;
        if (layer && layer->menu_open()) {
            switching_ = true;
            layer->close_menu();
            switching_ = false;
        }
    }
    if (root() && root()->modal() == this) root()->set_modal(nullptr);
    invalidate();
    if (on_mode_changed) on_mode_changed(false);
}

void MenuBar::on_modal_cancel() {
    // The root already cleared the modal; leave the rest of the state in step.
    if (open_ < 0) exit_mode();
}

// ---- Alt / F10 --------------------------------------------------------------------------------------

void MenuBar::hook(UiRoot& root) {
    root.on_alt_key = [this](bool down) { alt_changed(down); };
}

void MenuBar::alt_changed(bool down) {
    if (down) {
        alt_pending_ = true;
        if (!mode_) invalidate();
        return;
    }
    if (!alt_pending_) return;
    alt_pending_ = false;
    if (mode_) {
        exit_mode();
    } else {
        enter_mode();
    }
    invalidate();
}

bool MenuBar::preview_key(const KeyEvent& e) {
    if (e.vk == VK_SHIFT || e.vk == VK_CONTROL || e.vk == VK_MENU) return false;
    alt_pending_ = false;
    if (e.vk == VK_F10 && !e.mods.shift && !e.mods.ctrl && !e.mods.alt) {
        if (!allowed()) return false;
        if (mode_) {
            exit_mode();
        } else {
            enter_mode();
        }
        return true;
    }
    if (e.mods.alt && !e.mods.ctrl && ((e.vk >= 'A' && e.vk <= 'Z') || (e.vk >= '0' && e.vk <= '9'))) {
        const wchar_t ch = static_cast<wchar_t>(std::towlower(static_cast<wint_t>(e.vk)));
        for (size_t i = 0; i < menus_.size(); ++i) {
            if (parse_menu_label(menus_[i].text).key == ch) {
                if (!allowed()) return false;
                open(static_cast<int>(i), true);
                return true;
            }
        }
    }
    return false;
}

// ---- input ------------------------------------------------------------------------------------------

bool MenuBar::on_mouse_down(const MouseEvent& e) {
    const bool modal = root() && root()->modal() == this;
    const int h = heading_at(e.pt);
    if (h >= 0 && e.button == MouseButton::Left) {
        if (!allowed() && !mode_) return false;
        open(h, false, true);
        return true;
    }
    if (modal) {
        exit_mode();  // a press elsewhere leaves menu mode and still lands where it was aimed
        root()->mouse_down(e.pt, e.button, e.mods, e.clicks);
        return true;
    }
    return false;
}

bool MenuBar::on_mouse_up(const MouseEvent&) { return root() && root()->modal() == this; }

bool MenuBar::on_mouse_move(const MouseEvent& e) {
    const int h = heading_at(e.pt);
    const bool modal = root() && root()->modal() == this;
    if (modal) {
        if (h >= 0 && h != active_) {
            active_ = h;
            invalidate();
        }
        return true;
    }
    if (h != hover_) {
        hover_ = h;
        invalidate();
    }
    return false;
}

void MenuBar::on_mouse_leave() {
    if (hover_ >= 0) {
        hover_ = -1;
        invalidate();
    }
}

bool MenuBar::on_key_down(const KeyEvent& e) {
    if (!mode_ || open_ >= 0) return false;
    switch (e.vk) {
        case VK_LEFT: step(-1, false); break;
        case VK_RIGHT: step(1, false); break;
        case VK_DOWN:
        case VK_UP:
        case VK_RETURN:
        case VK_SPACE: open(active_ < 0 ? 0 : active_, true); break;
        case VK_ESCAPE:
        case VK_F10: exit_mode(); break;
        case VK_HOME: active_ = 0; invalidate(); break;
        case VK_END: active_ = static_cast<int>(menus_.size()) - 1; invalidate(); break;
        default: break;
    }
    return true;
}

bool MenuBar::on_char(wchar_t c) {
    if (!mode_ || open_ >= 0) return false;
    const wchar_t ch = static_cast<wchar_t>(std::towlower(c));
    std::vector<int> hits;
    for (size_t i = 0; i < menus_.size(); ++i) {
        if (parse_menu_label(menus_[i].text).key == ch) hits.push_back(static_cast<int>(i));
    }
    if (hits.size() == 1) {
        open(hits[0], true);
    } else if (hits.size() > 1) {
        int next = hits[0];
        for (int h : hits) {
            if (h > active_) {
                next = h;
                break;
            }
        }
        active_ = next;
        invalidate();
    }
    return true;
}

}  // namespace bb::ui
