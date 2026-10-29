// The Settings page: a layer over everything under the title bar (scrim, a card with the page list on the
// left and one page on the right). It owns no state: what is open, which page, every value and the lists
// the specialised pages edit (menus and toolbars, keyboard) are the core's, read on demand and written
// back at once; this rebuilds the page from them whenever a model moves.
#pragma once
#include "ui/layout.h"
#include "ui/scroll.h"
#include "views/command_ui.h"
#include "views/forms.h"
#include "views/model.h"

#include <memory>
#include <string>

namespace bb::views {

class MainView;

class SettingsView : public ui::Widget {
public:
    SettingsView(MainView& main, core::Core& core);

    // Reads the core's state: shows or hides the layer, switches page, rebuilds the page.
    void sync();
    bool is_open() const { return open_; }
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
    void choice_row(ui::Widget& p, const wchar_t* label, const char* key, std::vector<std::pair<std::wstring, std::string>> choices,
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
    bool command_editor_ = false;
    std::string command_editing_;  // the id of the user's command whose editor is open, or empty
    ui::Widget* card_ = nullptr;
    ui::StackPanel* nav_ = nullptr;
    ui::ScrollViewer* scroll_ = nullptr;
    ui::StackPanel* body_ = nullptr;
    // The strip over the page: why a list is not being saved (the core's), or why the last value was refused.
    ui::StackPanel* problem_strip_ = nullptr;
    ui::TextBlock* problem_ = nullptr;
    std::wstring refused_;
    void show_problem(const std::wstring& core_problem);
    json::Value pages_;
    // the editors' last models (for the harness and the keys page)
    json::Value bars_model_, keys_model_;
};

}  // namespace bb::views
