// The metadata pane of one tab: one box per field over whatever is selected, the file's own two dates, the
// cover picture, and what Windows can tell about the one file that nobody can type. Built the first time it
// is shown, like the preview.
//
// **It never writes.** Apply, the removals and the cover go through the core, which queues them and records
// them in the undo history; this only shows what the core's model says and tells it what was typed. The
// model is the core's (`bb_metadata_model`): which boxes there are, whether each is changed or invalid, and
// what Apply would do. After a write the list has reloaded, the pane has re-selected what it edited (the
// core does), and the boxes show what is on disk now.
#pragma once
#include "ui/controls.h"
#include "ui/edit.h"
#include "ui/layout.h"
#include "ui/pickers.h"
#include "ui/scroll.h"
#include "views/core.h"
#include "views/image_view.h"
#include "views/json.h"
#include "views/shell_icons.h"

#include <optional>

namespace bb::views {

class MetadataPane : public ui::DockPanel {
public:
    // `icons` draws the selected files' own icons in the list; without it the list shows names only.
    MetadataPane(core::Core& core, uint64_t tab, ShellIcons* icons = nullptr);

    // Points the pane at the tab's selection and reads it, unless neither the paths nor their modified times
    // moved (a refresh that changes neither must not wipe what is half typed). `force` always reads.
    void follow_selection(bool force = false);
    // Re-reads the core's model into the widgets (an apply finished, a box was typed into).
    void refresh();
    void set_active(bool active);

    const json::Value& model() const { return model_; }

    // ---- what the harness drives and reads ----
    // The text a box shows, "(multiple)" for a mixed one nobody has touched, "(none)" for an empty one, or
    // "(absent)" for a box this selection does not have.
    std::wstring field_text(const std::wstring& id) const;
    // Types into a box the way a person does: through a different value first, so emptying a box that started
    // empty still counts as typing in it.
    void type_into(const std::wstring& id, const std::wstring& value);
    // Presses Apply (0), remove location (1), remove everything (2), remove cover (3). Returns the operation.
    uint64_t press(int action);
    uint64_t set_cover(const std::vector<uint8_t>& image);
    void save_cover();
    void say_about_cover(const std::wstring& why);
    // The row of the file in the list, or null.
    const json::Value* file(const std::wstring& file_name) const;
    const json::Value* detail(const std::wstring& label) const;
    ui::Button* apply_button() const { return apply_; }
    ui::DockPanel* host() { return this; }

private:
    struct Row {
        std::string id;
        ui::StackPanel* box = nullptr;
        ui::Edit* edit = nullptr;
        ui::Button *revert = nullptr, *pick = nullptr;
        ui::TextBlock *label = nullptr, *note = nullptr;
        ui::Calendar* calendar = nullptr;
    };

    void rebuild();
    void update();
    std::string signature() const;
    void type_changed(const std::string& id, const std::wstring& text);
    void choose_cover();

    core::Core& core_;
    ShellIcons* icons_ = nullptr;
    uint64_t tab_;
    json::Value model_;
    std::string built_;
    bool active_ = false, suppress_ = false;
    std::wstring shown_selection_;

    ui::TextBlock *title_ = nullptr, *summary_ = nullptr, *empty_ = nullptr;
    ui::StackPanel* foot_ = nullptr;        // the summary and Apply: only while something is selected
    ui::StackPanel* remove_box_ = nullptr;  // the removals: the location note and its two buttons
    ui::Button *apply_ = nullptr, *remove_location_ = nullptr, *remove_all_ = nullptr, *choose_cover_ = nullptr,
               *remove_cover_ = nullptr;
    ui::ScrollViewer* scroll_ = nullptr;
    ui::StackPanel* column_ = nullptr;
    ui::StackPanel* files_ = nullptr;
    ui::StackPanel* fields_ = nullptr;
    ui::StackPanel* details_ = nullptr;
    ui::StackPanel* cover_box_ = nullptr;
    ImageView* cover_ = nullptr;
    ui::TextBlock *cover_note_ = nullptr, *location_note_ = nullptr;
    std::vector<Row> rows_;
};

}  // namespace bb::views
