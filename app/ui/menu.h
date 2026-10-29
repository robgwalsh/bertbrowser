// Menus: the item model, the panel that draws one level, and the controller that owns the open levels,
// the selection and the keyboard. None of it knows about windows - a MenuController talks to a
// PopupPresenter, so the same menu is drawn in-tree (PopupLayer), in owned popup windows, or into a
// bitmap for a photograph.
#pragma once
#include "ui/controls.h"
#include "ui/popup.h"

#include <string>
#include <vector>

namespace bb::ui {

struct MenuItem;
using MenuItems = std::vector<MenuItem>;

struct MenuItem {
    std::wstring text;     // "&Open": & marks the access key, "&&" is a literal ampersand
    std::wstring gesture;  // shortcut text, right aligned ("Ctrl+O")
    const IconData* icon = nullptr;
    bool is_separator = false;
    bool checked = false;  // a check mark, or a bullet when `radio`
    bool radio = false;
    bool enabled = true;
    bool marked = false;  // drawn as hovered without being the selection (the undo list's "back to here")
    int id = 0;
    std::function<void()> on_invoke;
    // The selection landed on this item (pointer or keys): may restyle the level it is in, which its
    // owner keeps alive while the menu is open (the undo list marks everything back to here).
    std::function<void(MenuItems& level, int index)> on_hover;
    MenuItems sub;  // non-empty = a submenu

    bool is_submenu() const { return !sub.empty(); }
    bool selectable() const { return !is_separator && enabled; }

    static MenuItem separator() {
        MenuItem m;
        m.is_separator = true;
        return m;
    }
    static MenuItem make(std::wstring text, std::function<void()> fn = {}, std::wstring gesture = {}) {
        MenuItem m;
        m.text = std::move(text);
        m.on_invoke = std::move(fn);
        m.gesture = std::move(gesture);
        return m;
    }
};

// ---- labels and access keys (pure) -----------------------------------------------------------------

struct MenuLabel {
    std::wstring display;  // the text with the markers removed
    int key_index = -1;    // index into `display` of the underlined character, or -1
    wchar_t key = 0;       // that character, lower-cased; 0 when there is none
};
MenuLabel parse_menu_label(const std::wstring& text);

// Selection helpers. `from` may be -1 (nothing selected). All skip separators and disabled items and wrap.
int menu_next_selectable(const MenuItems& items, int from, int dir);
int menu_first_selectable(const MenuItems& items);
int menu_last_selectable(const MenuItems& items);
// Enabled items whose access key is `ch` (case-insensitive), in order.
std::vector<int> menu_access_matches(const MenuItems& items, wchar_t ch);

// ---- the panel --------------------------------------------------------------------------------------

struct MenuMetrics {
    float item_h = 26.0f;
    float separator_h = 9.0f;
    float pad = 4.0f;          // inside the border, top and bottom and the sides
    float icon_col = 30.0f;    // width of the icon / check column
    float text_gap = 8.0f;
    float gesture_gap = 28.0f;
    float arrow_w = 22.0f;
    float min_width = 120.0f;
};

// One level of a menu. Not interactive itself: the controller hit-tests with item_at() and sets `selected`,
// which is how the same panel works in a popup window of its own and as a child of an owner's tree.
class MenuPanel : public Widget {
public:
    MenuPanel(gfx::Resources& res, const MenuItems& items, float min_width = 0);

    const MenuItems& items() const { return *items_; }
    int selected = -1;
    bool show_access = true;  // underline the access keys

    // Natural size of the panel, available before it is in any tree.
    Size natural_size() const { return natural_; }
    // Geometry is relative to the panel's own top-left: where the panel sits (in a popup window of its own,
    // or in the owner's tree) is the presenter's business, so nothing here reads `bounds`.
    Rect item_rect(int i) const;
    int item_at(Point local) const;  // -1 over padding, the border, or outside
    void set_selected(int i);

    Size measure(Size) override { return natural_; }
    void paint(PaintContext& pc) override;

    MenuMetrics metrics;

private:
    void compute();
    gfx::Resources& res_;
    const MenuItems* items_;
    float min_width_;
    Size natural_;
    std::vector<float> tops_;  // item i spans [tops_[i], tops_[i + 1]) down the panel
    std::vector<ComPtr<ID2D1PathGeometry>> icon_geo_;
    std::vector<uint32_t> icon_gen_;
};

// ---- the controller ---------------------------------------------------------------------------------

enum class MenuClose { Invoked, Dismissed, Escape };

class MenuController {
public:
    static constexpr unsigned kSubmenuDelayMs = 250;
    enum class Press { Outside, Inside };

    struct Level {
        const MenuItems* items = nullptr;
        MenuPanel* panel = nullptr;  // owned by the presenter
        Rect rect;
        int opener = -1;  // the item of the level below that opened this one
    };

    MenuController(gfx::Resources& res, PopupPresenter& presenter) : res_(res), pres_(presenter) {}
    ~MenuController() { teardown(); }
    MenuController(const MenuController&) = delete;
    MenuController& operator=(const MenuController&) = delete;

    // Opens `items` as the first level next to `anchor` (owner DIPs). `button_down`: the mouse button that
    // opened it is still held, so releasing over an item invokes it (press-drag-release on a menu bar).
    void open(const MenuItems& items, Rect anchor, PopupSide side = PopupSide::Below, float min_width = 0,
              bool select_first = false, bool button_down = false);
    // Closes every level. Fires on_closed once.
    void close(MenuClose why = MenuClose::Dismissed);
    bool is_open() const { return !levels_.empty(); }
    const std::vector<Level>& levels() const { return levels_; }
    int selected(size_t level) const { return level < levels_.size() ? levels_[level].panel->selected : -1; }
    // Select an item programmatically (tests, the bar): opens nothing.
    void select(size_t level, int index);
    // Opens the submenu of the selected item of `level`, selecting its first item when `first`.
    bool open_submenu(size_t level, bool first);
    void close_levels_from(size_t level);

    // Input, in owner DIPs.
    bool pointer_moved(Point p);  // true when p is over a level
    Press press(Point p);
    Press release(Point p);
    bool key(unsigned vk);       // always consumes; false only for keys the menu never takes
    bool character(wchar_t c);   // an access key
    void timer();                // the submenu hover delay fired

    bool show_access = true;
    std::function<void(const MenuItem&)> on_invoke;  // after the menu has closed
    std::function<void(MenuClose)> on_closed;
    // Left / Right pressed with only the root level open and nothing to open or close: a menu bar uses it to
    // move to the neighbouring heading. true = right.
    std::function<void(bool)> on_edge;

private:
    int level_at(Point p) const;  // topmost level whose rect holds p, or -1
    void push_level(const MenuItems& items, Rect anchor, PopupSide side, float min_width, int opener);
    void activate(size_t level, int index);
    void teardown();

    gfx::Resources& res_;
    PopupPresenter& pres_;
    std::vector<Level> levels_;
    int pending_level_ = -1, pending_item_ = -1;
    int pressed_level_ = -1, pressed_item_ = -1;
    bool button_held_ = false;
    bool closing_ = false;
};

}  // namespace bb::ui
