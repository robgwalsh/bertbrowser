// Commands on screen: the rows a menu or a toolbar is made of (read from the core's bars model), the
// menu items built from them, and the toolbar widget. The core decides what a row says, whether it is
// enabled and what it prints beside it; this only draws it and tells the window which command was picked.
#pragma once
#include "ui/menu.h"
#include "views/model.h"

#include <functional>
#include <string>
#include <vector>

namespace bb::views {

struct BarRow {
    std::string kind;  // command, menu, separator, list, placeholder
    std::string id;    // a command id, or a list id
    std::string icon;  // "Icon.Back"
    std::wstring caption, name, gesture, reason;
    bool checked = false, enabled = true, show_text = false;
    std::vector<BarRow> children;
};

BarRow read_bar_row(const json::Value& v);
std::vector<BarRow> read_bar_rows(const json::Value& rows);
// What a harness script calls a row in an assertion: the command id, "-", "@list" or "menu:caption".
std::string bar_token(const BarRow& r);
// The icon a core icon name ("Icon.Back") stands for; null when the set has none by that name.
const ui::IconData* icon_for(const std::string& name);
// A caption as it reads: access-key markers removed.
std::wstring plain_caption(const std::wstring& caption);
// Escapes a caption that came from data, so an & in it is not taken for an access key.
std::wstring escape_caption(const std::wstring& text);

struct MenuBuildEnv {
    core::Core* core = nullptr;
    std::function<void(const std::wstring& command_id)> run;
    // A row of a list (drives, workspaces, saved searches...) was picked.
    std::function<void(const std::string& list_id, const std::wstring& id, const std::wstring& name)> pick;
};

// Menu items for rows; a list row becomes the items it expands to right now.
ui::MenuItems build_menu_items(const MenuBuildEnv& env, const std::vector<BarRow>& rows);

// A strip of buttons. Items come from the core's `toolbar` model; the ones that do not fit sit behind an
// overflow button rather than being clipped.
class CommandToolbar : public ui::Widget {
public:
    static constexpr float kHeight = 32.0f;

    struct Item {
        BarRow row;
        float x = 0, w = 0;
    };

    explicit CommandToolbar(std::string which);
    const std::string& which() const { return which_; }

    // The core's answer for this toolbar. Buttons are updated in place unless the arrangement changed.
    void update(const json::Value& model);
    const std::vector<Item>& items() const { return items_; }
    size_t visible_count() const { return visible_; }
    bool shows_text() const { return show_text_; }

    // A button was clicked (its row) / the overflow button was (the rows behind it, the button's rect).
    std::function<void(const BarRow&, ui::Rect)> on_activate;
    std::function<void(const std::vector<BarRow>&, ui::Rect)> on_overflow;

    ui::Size measure(ui::Size avail) override;
    void arrange(ui::Rect r) override;
    void paint(ui::PaintContext& pc) override;
    ui::Widget* hit_test(ui::Point p) override;
    bool on_mouse_down(const ui::MouseEvent& e) override;
    bool on_mouse_up(const ui::MouseEvent& e) override;
    bool on_mouse_move(const ui::MouseEvent& e) override;
    void on_mouse_leave() override;

private:
    float item_width(const BarRow& r) const;
    void lay_out(float avail);
    int index_at(ui::Point p) const;  // an item index, kOverflow, or -1
    ui::Rect item_rect(size_t i) const;
    ui::Rect overflow_rect() const;
    static constexpr int kOverflow = -2;

    std::string which_;
    std::string signature_;
    std::vector<Item> items_;
    std::vector<ui::ComPtr<ID2D1PathGeometry>> icon_geo_;
    std::vector<uint32_t> icon_gen_;
    bool show_text_ = false;
    size_t visible_ = 0;
    float laid_width_ = -1;
    int hot_ = -1, pressed_ = -1;
};

}  // namespace bb::views
