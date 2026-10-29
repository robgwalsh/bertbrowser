#include "views/metadata_pane.h"
#include "views/file_pickers.h"

#include "gfx/icons.gen.h"
#include "views/pane_images.h"

#include <windows.h>

#include <commdlg.h>

#include <algorithm>

namespace bb::views {

using namespace bb::ui;

namespace {

void clear(Widget& w) {
    while (!w.children().empty()) w.remove_child(w.children().back().get());
}

TextBlock* small_text(Widget& parent, const std::wstring& text, Token fg = tok::TextSecondary) {
    auto* t = parent.emplace<TextBlock>(text);
    t->style.font.size = 11.0f;
    t->style.fg = fg;
    return t;
}

bool multiline_field(const std::string& id) { return id == "lyrics" || id == "comment"; }

}  // namespace

MetadataPane::MetadataPane(core::Core& core, uint64_t tab) : core_(core), tab_(tab) {
    name = "MetadataPane";
    style.bg = tok::SurfaceBackground;

    auto* header = emplace<StackPanel>(Orientation::Vertical, 1.0f);
    header->dock = Dock::Top;
    header->pad = 8;
    title_ = header->emplace<TextBlock>();
    title_->style.font.weight = 600;
    title_->name = "MetadataTitle";

    // The foot: what Apply would do, and the buttons.
    auto* foot = emplace<StackPanel>(Orientation::Vertical, 4.0f);
    foot->dock = Dock::Bottom;
    foot->pad = 8;
    summary_ = foot->emplace<TextBlock>();
    summary_->name = "MetadataSummary";
    summary_->wrap = true;
    summary_->style.font.size = 11.0f;
    summary_->style.fg = tok::TextSecondary;
    auto* buttons = foot->emplace<StackPanel>(Orientation::Horizontal, 6.0f);
    buttons->height = 30;
    apply_ = buttons->emplace<Button>(L"Apply");
    apply_->name = "MetadataApply";
    apply_->automation_id = "MetadataApply";
    apply_->primary = true;
    apply_->height = 28;
    apply_->on_click = [this] {
        try {
            press(0);
        } catch (const core::CoreError&) {
        }
    };

    scroll_ = emplace<ScrollViewer>();
    scroll_->dock = Dock::Fill;
    column_ = scroll_->emplace<StackPanel>(Orientation::Vertical, 10.0f);
    column_->pad = 8;

    empty_ = column_->emplace<TextBlock>(L"Select a file to see and change what it says about itself.");
    empty_->wrap = true;
    empty_->style.fg = tok::TextMuted;
    // Top to bottom as the C# pane has it: what nobody can type, the fields, the cover, the two removals,
    // then the selection. Spec: MetadataPane.xaml.
    details_ = column_->emplace<StackPanel>(Orientation::Vertical, 3.0f);
    details_->name = "MetadataDetails";
    fields_ = column_->emplace<StackPanel>(Orientation::Vertical, 8.0f);
    fields_->name = "MetadataFields";
    cover_box_ = column_->emplace<StackPanel>(Orientation::Vertical, 4.0f);
    cover_box_->name = "MetadataCover";
    // Removing: buttons that act at once, each a whole request on its own and one undo away from never
    // having happened.
    remove_box_ = column_->emplace<StackPanel>(Orientation::Vertical, 6.0f);
    remove_box_->name = "MetadataRemove";
    location_note_ = remove_box_->emplace<TextBlock>();
    location_note_->name = "MetadataLocationNote";
    location_note_->wrap = true;
    location_note_->style.font.size = 11.0f;
    location_note_->style.fg = tok::TextSecondary;
    auto* removals = remove_box_->emplace<StackPanel>(Orientation::Horizontal, 6.0f);
    remove_location_ = removals->emplace<Button>(L"Remove location");
    remove_location_->name = "MetadataRemoveLocation";
    remove_location_->height = 28;
    remove_location_->tooltip = L"Take the GPS position out of these pictures and leave everything else.";
    remove_location_->on_click = [this] {
        try {
            press(1);
        } catch (const core::CoreError&) {
        }
    };
    remove_all_ = removals->emplace<Button>(L"Remove all metadata");
    remove_all_->name = "MetadataRemoveAll";
    remove_all_->height = 28;
    remove_all_->tooltip = L"Take out everything that describes these files. A picture keeps its rotation and colour profile.";
    remove_all_->on_click = [this] {
        try {
            press(2);
        } catch (const core::CoreError&) {
        }
    };
    files_ = column_->emplace<StackPanel>(Orientation::Vertical, 2.0f);
    files_->name = "MetadataFiles";
}

void MetadataPane::set_active(bool active) {
    active_ = active;
    if (active) follow_selection(false);
}

void MetadataPane::follow_selection(bool force) {
    if (!active_ && !force) return;
    core::check(bb_metadata_load(core_.app(), tab_, force), "metadata load");
    refresh();
}

void MetadataPane::refresh() {
    BbOwnedStr o{};
    if (bb_metadata_model(core_.app(), tab_, &o) != BB_OK) return;
    model_ = json::parse(core::utf8(core::take(o)));
    if (signature() != built_) rebuild();
    update();
}

std::string MetadataPane::signature() const {
    std::string s;
    for (size_t i = 0; i < model_["files"].size(); ++i) s += model_["files"][i]["path"].str() + "|" + model_["files"][i]["state"].str() + "|" + model_["files"][i]["detail"].str() + ";";
    s += "#";
    for (size_t i = 0; i < model_["fields"].size(); ++i) s += model_["fields"][i]["id"].str() + (model_["fields"][i]["isMixed"].boolean() ? "m" : "") + ";";
    s += "#" + std::to_string(model_["details"].size()) + std::string(model_["canHoldPicture"].boolean() ? "c" : "") + (model_["hasPicture"].boolean() ? "p" : "");
    s += model_["pictureNote"].str() + std::to_string(model_["located"].i64());
    return s;
}

void MetadataPane::rebuild() {
    built_ = signature();
    rows_.clear();
    clear(*files_);
    clear(*cover_box_);
    clear(*fields_);
    clear(*details_);

    const json::Value& m = model_;
    const bool empty = m["isEmpty"].boolean();
    empty_->set_visible(empty);

    for (size_t i = 0; i < m["files"].size() && i < 8; ++i) {
        const json::Value& f = m["files"][i];
        std::wstring line = core::wide(f["name"].str());
        const std::wstring detail = core::wide(f["detail"].str());
        const std::string state = f["state"].str();
        if (state == "Changed") line += L"  ·  changed";
        else if (state == "Failed") line += L"  ·  failed";
        auto* t = small_text(*files_, line, state == "Failed" ? tok::ErrorForeground : tok::TextPrimary);
        t->name = "MetadataFile";
        if (!detail.empty()) {
            auto* d = small_text(*files_, detail, state == "Failed" ? tok::ErrorForeground : tok::TextMuted);
            d->wrap = true;
        }
    }
    if (m["files"].size() > 8) small_text(*files_, L"and " + std::to_wstring(m["files"].size() - 8) + L" more", tok::TextMuted);

    // The cover picture.
    if (m["canHoldPicture"].boolean()) {
        small_text(*cover_box_, L"Cover picture");
        auto* frame = cover_box_->emplace<Panel>();
        frame->height = 132;
        frame->width = 176;
        cover_ = frame->emplace<ImageView>();
        cover_->interactive = false;
        cover_->name = "MetadataCoverImage";
        if (m["hasPicture"].boolean()) {
            size_t length = 0;
            bb_metadata_picture(core_.app(), tab_, nullptr, 0, &length);
            std::vector<uint8_t> bytes(length);
            if (length && bb_metadata_picture(core_.app(), tab_, bytes.data(), bytes.size(), &length) == BB_OK) {
                cover_->set_image(decode_picture(bytes, 320));
            }
        } else {
            cover_->set_image(nullptr);
        }
        cover_note_ = small_text(*cover_box_, core::wide(m["pictureNote"].str()), tok::TextMuted);
        auto* row = cover_box_->emplace<StackPanel>(Orientation::Horizontal, 6.0f);
        row->height = 28;
        choose_cover_ = row->emplace<Button>(L"Choose…");
        choose_cover_->name = "MetadataChooseCover";
        choose_cover_->height = 26;
        choose_cover_->on_click = [this] { choose_cover(); };
        auto* save = row->emplace<Button>(L"Save as\u2026");
        save->name = "MetadataSaveCover";
        save->height = 26;
        save->set_enabled(m["hasPicture"].boolean());
        save->on_click = [this] { save_cover(); };
        remove_cover_ = row->emplace<Button>(L"Remove");
        remove_cover_->name = "MetadataRemoveCover";
        remove_cover_->height = 26;
        remove_cover_->on_click = [this] {
            try {
                press(3);
            } catch (const core::CoreError&) {
            }
        };
    } else {
        cover_ = nullptr;
        cover_note_ = nullptr;
        choose_cover_ = remove_cover_ = nullptr;
    }
    cover_box_->set_visible(m["canHoldPicture"].boolean());

    // The boxes.
    for (size_t i = 0; i < m["fields"].size(); ++i) {
        const json::Value& f = m["fields"][i];
        Row row;
        row.id = f["id"].str();
        row.box = fields_->emplace<StackPanel>(Orientation::Vertical, 2.0f);
        row.box->name = "MetadataField_" + row.id;
        row.label = small_text(*row.box, core::wide(f["label"].str()));
        auto* line = row.box->emplace<StackPanel>(Orientation::Horizontal, 4.0f);
        const bool multi = multiline_field(row.id);
        line->height = multi ? 72.0f : 28.0f;
        row.edit = line->emplace<Edit>(multi);
        row.edit->name = "MetadataEdit_" + row.id;
        row.edit->flex = 1;
        row.edit->height = multi ? 72.0f : 28.0f;
        row.edit->tooltip = f["hint"].is_null() ? L"" : core::wide(f["hint"].str());
        const std::string id = row.id;
        row.edit->on_changed = [this, id](const std::wstring& text) { type_changed(id, text); };
        if (f["isDate"].boolean()) {
            row.pick = line->emplace<Button>(L"▾");
            row.pick->name = "MetadataPick_" + row.id;
            row.pick->width = 28;
            row.pick->height = 28;
            row.pick->tooltip = L"Pick a date";
        }
        row.revert = line->emplace<Button>();
        row.revert->name = "MetadataRevert_" + row.id;
        row.revert->icon = icons::find("Undo");
        row.revert->width = 28;
        row.revert->height = 28;
        row.revert->tooltip = L"Revert";
        row.revert->on_click = [this, id] {
            bb_metadata_revert(core_.app(), tab_, core::s(core::wide(id)));
            refresh();
        };
        row.note = small_text(*row.box, L"", tok::TextSecondary);
        if (row.pick) {
            row.calendar = row.box->emplace<Calendar>();
            row.calendar->set_visible(false);
            Calendar* cal = row.calendar;
            row.calendar->on_select = [this, id, cal](Date d) {
                bb_metadata_pick_date(core_.app(), tab_, core::s(core::wide(id)), d.year, static_cast<uint32_t>(d.month), static_cast<uint32_t>(d.day));
                cal->set_visible(false);
                refresh();
            };
            row.pick->on_click = [cal] { cal->set_visible(!cal->flags.visible); };
        }
        rows_.push_back(std::move(row));
    }

    // What nobody can type.
    if (m["details"].size() > 0) {
        small_text(*details_, L"Details");
        for (size_t i = 0; i < m["details"].size(); ++i) {
            auto* line = details_->emplace<StackPanel>(Orientation::Horizontal, 8.0f);
            line->height = 16;
            auto* label = small_text(*line, core::wide(m["details"][i]["label"].str()), tok::TextMuted);
            label->width = 96;
            small_text(*line, core::wide(m["details"][i]["value"].str()), tok::TextPrimary)->flex = 1;
        }
    }
    request_layout();
    invalidate();
}

void MetadataPane::update() {
    const json::Value& m = model_;
    title_->set_text(core::wide(m["title"].str()));
    summary_->set_text(core::wide(m["summary"].str()));
    apply_->set_enabled(m["canApply"].boolean());
    remove_box_->set_visible(m["canRemove"].boolean());
    remove_all_->set_enabled(m["canRemoveAll"].boolean());
    remove_location_->set_enabled(m["canRemoveLocation"].boolean());
    const std::wstring note = core::wide(m["locationNote"].str());
    location_note_->set_text(note);
    location_note_->set_visible(!note.empty());
    if (remove_cover_) remove_cover_->set_enabled(m["canRemovePicture"].boolean());

    suppress_ = true;
    for (Row& row : rows_) {
        const json::Value* f = nullptr;
        for (size_t i = 0; i < m["fields"].size(); ++i) {
            if (m["fields"][i]["id"].str() == row.id) f = &m["fields"][i];
        }
        if (!f) continue;
        const std::wstring text = core::wide((*f)["text"].str());
        if (row.edit->text() != text) row.edit->set_text(text);
        row.edit->placeholder = (*f)["mixedPlaceholder"].boolean() ? L"(multiple)" : L"";
        row.revert->set_visible((*f)["isDirty"].boolean());
        const bool invalid = (*f)["isInvalid"].boolean();
        row.note->set_text(core::wide((*f)["note"].str()));
        row.note->style.fg = invalid ? tok::ErrorForeground : tok::TextSecondary;
        row.note->set_visible((*f)["showsNote"].boolean());
        row.edit->style.border = invalid ? tok::ErrorBorder : kNoToken;
        if (row.calendar && !(*f)["pickedDate"].is_null()) {
            const std::string d = (*f)["pickedDate"].str();
            if (d.size() == 10) row.calendar->set_selected(Date{std::atoi(d.substr(0, 4).c_str()), std::atoi(d.substr(5, 2).c_str()), std::atoi(d.substr(8, 2).c_str())});
        }
    }
    suppress_ = false;
    request_layout();
    invalidate();
}

void MetadataPane::type_changed(const std::string& id, const std::wstring& text) {
    if (suppress_) return;
    bb_metadata_set_text(core_.app(), tab_, core::s(core::wide(id)), core::s(text));
    // The note, the summary and Apply follow the box; the boxes themselves stay as typed.
    BbOwnedStr o{};
    if (bb_metadata_model(core_.app(), tab_, &o) == BB_OK) {
        model_ = json::parse(core::utf8(core::take(o)));
        update();
    }
}

std::wstring MetadataPane::field_text(const std::wstring& id) const {
    const std::string want = core::utf8(id);
    for (size_t i = 0; i < model_["fields"].size(); ++i) {
        const json::Value& f = model_["fields"][i];
        if (!core::iequals(core::wide(f["id"].str()), id)) continue;
        if (f["mixedPlaceholder"].boolean()) return L"(multiple)";
        const std::wstring text = core::wide(f["text"].str());
        return text.empty() ? L"(none)" : text;
    }
    return L"(absent)";
}

void MetadataPane::type_into(const std::wstring& id, const std::wstring& value) {
    core::check(bb_metadata_set_text(core_.app(), tab_, core::s(id), core::s(value + L" ")), "metadata type");
    core::check(bb_metadata_set_text(core_.app(), tab_, core::s(id), core::s(value)), "metadata type");
    refresh();
}

uint64_t MetadataPane::press(int action) {
    uint64_t op = 0;
    core::check(bb_metadata_act(core_.app(), tab_, static_cast<uint32_t>(action), nullptr, 0, &op), "metadata");
    return op;
}

uint64_t MetadataPane::set_cover(const std::vector<uint8_t>& image) {
    uint64_t op = 0;
    core::check(bb_metadata_act(core_.app(), tab_, 4, image.data(), image.size(), &op), "metadata picture");
    return op;
}

// The picker is the view's job; embedding what it picked is the core's. A cover is embedded whole in every
// selected file, so one of tens of megabytes was chosen by mistake. Spec: MetadataPane.ChoosePicture_Click.
void MetadataPane::choose_cover() {
    PickPath pick;
    pick.title = L"Choose a cover picture";
    pick.filters = {{L"Pictures", L"*.jpg;*.jpeg;*.png"}, {L"All files", L"*.*"}};
    const auto chosen = pick_open_file(GetActiveWindow(), pick);
    if (!chosen) return;
    WIN32_FILE_ATTRIBUTE_DATA data{};
    if (GetFileAttributesExW(chosen->c_str(), GetFileExInfoStandard, &data) &&
        ((static_cast<uint64_t>(data.nFileSizeHigh) << 32) | data.nFileSizeLow) > (16ull << 20)) {
        say_about_cover(L"That picture is larger than 16 MB, which is too large to embed as a cover.");
        return;
    }
    std::vector<uint8_t> bytes;
    if (!read_whole_file(*chosen, 16ull << 20, bytes)) {
        say_about_cover(L"The picture could not be read.");
        return;
    }
    try {
        set_cover(bytes);
    } catch (const core::CoreError& e) {
        say_about_cover(core::wide(e.what()));
    }
}

// Writes the cover out as a file of its own, named for what the bytes are (the picture's business, not the
// song's). Spec: MetadataPane.SavePicture_Click.
void MetadataPane::save_cover() {
    size_t length = 0;
    bb_metadata_picture(core_.app(), tab_, nullptr, 0, &length);
    if (!length) return;
    std::vector<uint8_t> bytes(length);
    if (bb_metadata_picture(core_.app(), tab_, bytes.data(), bytes.size(), &length) != BB_OK) return;
    const bool png = bytes.size() >= 4 && bytes[0] == 0x89 && bytes[1] == 0x50 && bytes[2] == 0x4E && bytes[3] == 0x47;
    PickPath pick;
    pick.title = L"Save cover picture";
    pick.file_name = png ? L"cover.png" : L"cover.jpg";
    pick.default_ext = png ? L"png" : L"jpg";
    pick.filters = {png ? std::pair<std::wstring, std::wstring>{L"PNG picture", L"*.png"} : std::pair<std::wstring, std::wstring>{L"JPEG picture", L"*.jpg"}};
    const auto target = pick_save_file(GetActiveWindow(), pick);
    if (!target) return;
    HANDLE h = CreateFileW(target->c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    DWORD written = 0;
    const bool ok = h != INVALID_HANDLE_VALUE && WriteFile(h, bytes.data(), static_cast<DWORD>(bytes.size()), &written, nullptr) && written == bytes.size();
    if (h != INVALID_HANDLE_VALUE) CloseHandle(h);
    if (!ok) say_about_cover(L"The picture could not be saved.");
}

// Why the last cover action did nothing, under the picture until the pane is read again.
void MetadataPane::say_about_cover(const std::wstring& why) {
    if (!cover_note_) return;
    cover_note_->set_text(why);
    cover_note_->style.fg = tok::ErrorForeground;
    cover_note_->invalidate();
}

const json::Value* MetadataPane::file(const std::wstring& file_name) const {
    for (size_t i = 0; i < model_["files"].size(); ++i) {
        if (core::iequals(core::wide(model_["files"][i]["name"].str()), file_name)) return &model_["files"][i];
    }
    return nullptr;
}

const json::Value* MetadataPane::detail(const std::wstring& label) const {
    for (size_t i = 0; i < model_["details"].size(); ++i) {
        if (core::icontains(core::wide(model_["details"][i]["label"].str()), label)) return &model_["details"][i];
    }
    return nullptr;
}

}  // namespace bb::views
