// The Settings page: a layer over everything under the title bar (scrim, a card with the page list on the
// left and one page on the right). It owns no state: what is open, which page, every value and the lists
// the specialised pages edit (menus and toolbars, keyboard) are the core's, read on demand and written
// back at once; this rebuilds the page from them whenever a model moves.
#pragma once
#include "ui/core/layout.h"
#include "ui/core/scroll.h"
#include "views/chrome/command_ui.h"
#include "views/dialogs/forms.h"
#include "views/bridge/model.h"

#include <functional>
#include <memory>
#include <string>

namespace bb::views {

class MainView;
class LeaveLayer;

class SettingsView : public ui::Widget {
public:
    SettingsView(MainView& main, core::Core& core);

    // Reads the core's state: shows or hides the layer, switches page, rebuilds the page.
    void sync();
    bool is_open() const { return open_; }
    // Esc goes back; on the Keyboard page the next key is the recorder's answer. Asked by the main view
    // before the focused control sees a key; true = taken.
    bool preview_key(const ui::KeyEvent& e);
    const std::string& page_id() const { return page_; }

    // ---- what the harness asks ----
    ui::CheckBox* find_check(const std::wstring& label);
    // The Menus and toolbars page's rows / picker, as the page shows them.
    std::vector<std::wstring> bar_rows() const;
    std::vector<std::wstring> bar_choices() const;
    // The Keyboard page's recorder line and one row.
    std::wstring recorder_text() const;
    bool key_row(const std::wstring& label, std::vector<std::wstring>* chords) const;
    bool dragging_pose() const { return dragging_pose_; }
    // The Context menu page's New command button, pressed (the editor it opens is what gets photographed).
    void open_new_command();
    void pose_column_drag(bool on);
    ui::Widget* body() { return body_; }

    ui::Size measure(ui::Size avail) override { return avail; }
    void arrange(ui::Rect r) override;
    void paint(ui::PaintContext& pc) override;
    bool on_mouse_down(const ui::MouseEvent& e) override;
    bool on_wheel(ui::Point, int, ui::Mods) override { return true; }

private:
    void build_chrome();
    // Back to the folders. A list the page could not save is refused by the core, which brings its page to
    // the front; this then asks whether to leave anyway, and `then` runs once the page has really closed.
    void go_back(std::function<void()> then = nullptr);
    void ask_leave(const std::wstring& why, std::function<void()> then);
    // Leave anyway: the unsaved lists go (the page's copies, and the core's hold on them), then the page closes.
    void leave_unsaved(const std::function<void()>& then);
    // The theme editor is a window of its own, so the page gets out of the way first. Spec: MainWindow.CustomiseTheme.
    void customise_colours();
    void build_nav(const json::Value& pages);
    void build_page();
    void rebuild_if_changed(bool force);
    // pages
    void page_general(ui::Widget& p);
    void page_appearance(ui::Widget& p);
    void page_preview(ui::Widget& p);
    void page_search_index(ui::Widget& p);
    void page_history(ui::Widget& p);
    void page_new_items(ui::Widget& p);
    // The New items page edits its own copy of the list, written back whole on every change, so typing in
    // the editor never rebuilds the page under the caret. Read from the core on arrival and after an import.
    struct NewType {
        std::wstring label, extension, template_path;
        bool enabled = true;
    };
    void load_new_types();
    void write_new_types();
    void refresh_new_type_row(int i);
    std::vector<NewType> new_types_;
    int new_type_selected_ = -1;
    bool new_types_loaded_ = false;
    bool new_types_importing_ = false;  // this page asked for an import that has not been read back yet
    ui::Widget* new_type_list_ = nullptr;
    void page_columns(ui::Widget& p);
    int column_selected_ = 0;
    void page_saved(ui::Widget& p, bool workspaces);
    void page_context_menu(ui::Widget& p);
    void page_bars(ui::Widget& p);
    // The bars page's two inline name boxes: Save as (a new name) and Rename (a saved one's).
    enum class BarNaming { None, SaveAs, Rename };
    BarNaming bar_naming_ = BarNaming::None;
    std::wstring bar_name_text_;
    void page_keys(ui::Widget& p);

    // settings values
    json::Value get(const char* key);
    void set_raw(const char* key, const std::string& json_value);
    void set_bool(const char* key, bool v) { set_raw(key, v ? "true" : "false"); }
    void set_text(const char* key, const std::wstring& v, bool null_if_empty = false);
    void number_row(ui::Widget& p, const wchar_t* label, const char* key, const wchar_t* unit = L"");
    void bool_row(ui::Widget& p, const wchar_t* label, const char* key);
    // A setting with a fixed list of values, as the original's combos and sliders had: a value off the list
    // (a hand-edited file) is shown as itself rather than rewritten. `choices` are label and JSON value.
    ui::ComboBox* choice_row(ui::Widget& p, const wchar_t* label, const char* key, std::vector<std::pair<std::wstring, std::string>> choices,
                    const char* when_unset = nullptr);
    // What a hint calls a command: its shortcut, or its name when it has none.
    std::wstring gesture_or(const wchar_t* command, const wchar_t* fallback);

    MainView& main_;
    core::Core& core_;
    bool open_ = false;
    std::string page_;
    std::string signature_;
    std::wstring folder_handler_warning_;  // why the last change to the folder handler did not happen
    bool dragging_pose_ = false;
    int menu_selected_ = -1;
    // The Context menu page's commands as typed, while the core holds them unwritten for a missing field: a
    // JSON array, or null. The page re-reads the saved list otherwise, which would drop the half-typed command.
    json::Value pending_commands_;
    bool reveal_problem_ = false;  // a refused leave asked the page to open the command that is incomplete
    bool command_editor_ = false;
    std::string command_editing_;  // the id of the user's command whose editor is open, or empty
    ui::Widget* card_ = nullptr;
    LeaveLayer* leave_ = nullptr;  // the "leave anyway?" card, over the page
    ui::StackPanel* nav_ = nullptr;
    ui::ScrollViewer* scroll_ = nullptr;
    ui::StackPanel* body_ = nullptr;
    // The strip over the page: why a list is not being saved (the core's), or why the last value was refused.
    ui::StackPanel* problem_strip_ = nullptr;
    ui::TextBlock* problem_ = nullptr;
    std::wstring refused_;
    void show_problem(const std::wstring& core_problem);
    json::Value pages_;
    // Alt+Up/Down on a list a page builds (the menu layout, the bar rows): the page sets what moves the
    // selected row, and `preview_key` calls it while that list has the keyboard. Cleared with the page.
    std::function<void(int)> nudge_;
    ui::Widget* nudge_for_ = nullptr;
    // the editors' last models (for the harness and the keys page)
    json::Value bars_model_, keys_model_;
};

}  // namespace bb::views
