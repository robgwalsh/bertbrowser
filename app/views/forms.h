// Small builders for forms (the Settings pages, dialogs): a heading, a note, a row of controls, a list of
// selectable rows. Everything is a widget in the parent's tree; nothing here knows about the core.
#pragma once
#include "ui/controls.h"
#include "ui/edit.h"
#include "ui/inputs.h"
#include "ui/layout.h"
#include "ui/scroll.h"

#include <functional>
#include <string>
#include <vector>

namespace bb::views::forms {

using namespace bb::ui;

TextBlock* text(Widget& parent, std::wstring t, Token fg = tok::TextPrimary, float size = 14.0f, int weight = 400);
TextBlock* heading(Widget& parent, std::wstring t);
// Muted wrapped text.
TextBlock* note(Widget& parent, std::wstring t);
StackPanel* row(Widget& parent, float gap = 8.0f);
StackPanel* column(Widget& parent, float gap = 8.0f);
Widget* gap(Widget& parent, float h);
// A label at the left of a row, a fixed width so the controls after it line up.
TextBlock* row_label(StackPanel& row, std::wstring t, float width = 220.0f);
CheckBox* check(Widget& parent, std::wstring label, bool value, std::function<void(bool)> on_toggle);
Button* button(Widget& parent, std::wstring label, std::function<void()> on_click, float width = -1.0f);
Edit* field(Widget& parent, std::wstring value, float width, std::function<void(const std::wstring&)> on_changed,
            std::wstring placeholder = {});
ComboBox* combo(Widget& parent, std::vector<std::wstring> items, int selected, float width, std::function<void(int)> on_change);
RadioGroup* radios(Widget& parent, std::vector<std::wstring> items, int selected, std::function<void(int)> on_change);

struct ListEntry {
    std::wstring text, detail;
    const IconData* icon = nullptr;
    // A picture instead of `icon` (a shell icon), asked for at paint time since it may still be loading.
    std::function<ThumbPtr(uint32_t px)> picture;
    bool faded = false;  // the picture drawn dimmed (a hidden file)
    bool heading = false;  // a group title: not selectable
    bool separator = false;
    bool enabled = true;
    bool muted = false;
    bool swatch_on = false;  // a small square of colour before the text (the theme editor)
    uint32_t swatch = 0;     // 0xAARRGGBB
};

// A scrolling list of rows with one selected. Rows have a fixed height, so painting asks for the visible ones.
class ItemList : public Widget {
public:
    ItemList() {
        flags.focusable = true;
        flags.tab_stop = true;
        style.bg = tok::InputBackground;
        style.border = tok::BorderSubtle;
    }
    float row_height = 26.0f;
    std::vector<ListEntry> entries;
    int selected = -1;
    std::function<void(int)> on_select;    // a click or a key moved the selection
    std::function<void(int)> on_activate;  // a double click or Enter
    std::function<void(int, Point)> on_context;  // a right click on a row, which does not select it

    void set_entries(std::vector<ListEntry> e, int select = -1);
    void select(int i, bool notify = false);
    void scroll_into_view(int i);
    Rect row_rect(int i) const;
    int row_at(Point p) const;
    int visible_rows() const;

    Size measure(Size avail) override;
    void arrange(Rect r) override;
    void paint(PaintContext& pc) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_mouse_up(const MouseEvent& e) override;
    bool on_mouse_move(const MouseEvent& e) override;
    bool on_wheel(Point p, int delta, Mods m) override;
    bool on_key_down(const KeyEvent& e) override;
    void on_focus(bool) override { invalidate(); }

private:
    int step(int from, int dir) const;
    void layout_bar();
    ScrollBar v_{true};
    ImageCache images_;
    std::vector<ComPtr<ID2D1PathGeometry>> icon_geo_;
    std::vector<uint32_t> icon_gen_;
};

}  // namespace bb::views::forms
