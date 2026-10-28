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
    void page_columns(ui::Widget& p);
    void page_saved(ui::Widget& p, bool workspaces);
    void page_context_menu(ui::Widget& p);
    void page_bars(ui::Widget& p);
    void page_keys(ui::Widget& p);

    // settings values
    json::Value get(const char* key);
    void set_raw(const char* key, const std::string& json_value);
    void set_bool(const char* key, bool v) { set_raw(key, v ? "true" : "false"); }
    void set_text(const char* key, const std::wstring& v, bool null_if_empty = false);
    void number_row(ui::Widget& p, const wchar_t* label, const char* key, const wchar_t* unit = L"");
    void bool_row(ui::Widget& p, const wchar_t* label, const char* key);

    MainView& main_;
    core::Core& core_;
    bool open_ = false;
    std::string page_;
    std::string signature_;
    bool dragging_pose_ = false;
    int menu_selected_ = -1;
    bool command_editor_ = false;
    ui::Widget* card_ = nullptr;
    ui::StackPanel* nav_ = nullptr;
    ui::ScrollViewer* scroll_ = nullptr;
    ui::StackPanel* body_ = nullptr;
    ui::TextBlock* problem_ = nullptr;
    json::Value pages_;
    // the editors' last models (for the harness and the keys page)
    json::Value bars_model_, keys_model_;
};

}  // namespace bb::views
