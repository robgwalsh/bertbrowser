#include "views/panes/preview_pane.h"

#include "gfx/icons.gen.h"
#include "views/panes/media_pane.h"

#include <windows.h>

#include <algorithm>
#include <cmath>
#include <cwctype>
#include <list>
#include <unordered_map>

namespace bb::views {

using namespace bb::ui;

namespace {

bool ends_with_nocase(const std::wstring& s, const wchar_t* suffix) {
    const size_t n = wcslen(suffix);
    return s.size() >= n && _wcsicmp(s.c_str() + s.size() - n, suffix) == 0;
}

std::wstring thousands(int64_t n) {
    std::wstring digits = std::to_wstring(n < 0 ? -n : n), out;
    for (size_t i = 0; i < digits.size(); ++i) {
        if (i && (digits.size() - i) % 3 == 0) out += L',';
        out += digits[i];
    }
    return n < 0 ? L"-" + out : out;
}

std::wstring byte_size(int64_t bytes) {
    if (bytes < 0) return L"—";
    static const wchar_t* units[] = {L"B", L"KB", L"MB", L"GB", L"TB"};
    double v = static_cast<double>(bytes);
    int u = 0;
    while (v >= 1024.0 && u < 4) {
        v /= 1024.0;
        ++u;
    }
    wchar_t buf[48];
    if (u == 0) swprintf(buf, 48, L"%lld B", static_cast<long long>(bytes));
    else swprintf(buf, 48, v < 10 ? L"%.2f %ls" : v < 100 ? L"%.1f %ls" : L"%.0f %ls", v, units[u]);
    return buf;
}

std::wstring join(std::initializer_list<std::wstring> parts) {
    std::wstring out;
    for (const auto& p : parts) {
        if (p.empty()) continue;
        if (!out.empty()) out += L"  ·  ";
        out += p;
    }
    return out;
}

// The shell's answers for the last 50 files (path, size and modified time), so arrowing back over documents is
// instant. A refusal is remembered too: it stays one until the file changes. Spec: Interop/PreviewImageCache.
ui::ThumbPtr cached_shell_preview(const std::wstring& path, uint32_t px) {
    constexpr size_t kCapacity = 50;
    static std::mutex gate;
    static std::list<std::pair<std::wstring, ui::ThumbPtr>> order;  // most recent first
    static std::unordered_map<std::wstring, std::list<std::pair<std::wstring, ui::ThumbPtr>>::iterator> map;

    WIN32_FILE_ATTRIBUTE_DATA info{};
    if (!GetFileAttributesExW(path.c_str(), GetFileExInfoStandard, &info)) return shell_preview(path, px);
    std::wstring key = path;
    CharUpperBuffW(key.data(), static_cast<DWORD>(key.size()));
    key += L"|" + std::to_wstring(px) + L"|" + std::to_wstring((static_cast<uint64_t>(info.nFileSizeHigh) << 32) | info.nFileSizeLow) + L"|" +
           std::to_wstring((static_cast<uint64_t>(info.ftLastWriteTime.dwHighDateTime) << 32) | info.ftLastWriteTime.dwLowDateTime);
    {
        std::lock_guard<std::mutex> lock(gate);
        if (auto hit = map.find(key); hit != map.end()) {
            order.splice(order.begin(), order, hit->second);
            return hit->second->second;
        }
    }
    ui::ThumbPtr image = shell_preview(path, px);
    std::lock_guard<std::mutex> lock(gate);
    if (auto hit = map.find(key); hit != map.end()) {
        order.splice(order.begin(), order, hit->second);
        return hit->second->second;
    }
    order.emplace_front(key, image);
    map[key] = order.begin();
    if (map.size() > kCapacity) {
        map.erase(order.back().first);
        order.pop_back();
    }
    return image;
}

Button* make_button(Widget& parent, const wchar_t* label, const char* name, std::function<void()> click) {
    auto* b = parent.emplace<Button>(label);
    b->name = name;
    b->on_click = std::move(click);
    b->height = 26;
    return b;
}

}  // namespace

// The rows of an archive's listing.
struct PreviewPane::ArchiveRows : ui::RowProvider {
    struct Row {
        std::wstring path, size, packed;
    };
    std::vector<Row> rows;
    size_t row_count() const override { return rows.size(); }
    uint64_t row_id(size_t row) const override { return row; }
    std::wstring cell_text(size_t row, int column) const override {
        if (row >= rows.size()) return {};
        return column == 0 ? rows[row].path : column == 1 ? rows[row].size : rows[row].packed;
    }
    bool selected(size_t) const override { return false; }
    void select(ui::SelectOp, size_t, size_t) override {}
    std::wstring row_tip(size_t row) const override { return row < rows.size() ? rows[row].path : std::wstring(); }
};

PreviewPane::PreviewPane(core::Core& core, uint64_t tab, HWND window) : core_(core), tab_(tab), window_(window) {
    name = "PreviewPane";
    style.bg = tok::SurfaceBackground;

    // ---- header: the name, then type / size / modified, and the fit-width button beside it ----
    auto* header = emplace<DockPanel>();
    header->dock = Dock::Top;
    header->height = 50;
    header->pad = 8;
    fit_width_ = header->emplace<Button>();
    fit_width_->name = "FitWidthButton";
    fit_width_->automation_id = "FitWidthButton";
    fit_width_->dock = Dock::Right;
    fit_width_->width = 30;
    fit_width_->height = 28;
    fit_width_->icon = icons::find("FitWidth");
    fit_width_->tooltip = L"Expand the pane to fit its contents";
    fit_width_->on_click = [this] {
        if (on_fit_width) on_fit_width();
    };
    auto* texts = header->emplace<StackPanel>(Orientation::Vertical, 1.0f);
    texts->dock = Dock::Fill;
    title_ = texts->emplace<TextBlock>();
    title_->style.font.weight = 600;
    subtitle_ = texts->emplace<TextBlock>();
    subtitle_->style.font.size = 11.0f;
    subtitle_->style.fg = tok::TextSecondary;

    // ---- per-kind actions ----
    actions_ = emplace<StackPanel>(Orientation::Horizontal, 4.0f);
    actions_->dock = Dock::Top;
    actions_->height = 32;
    actions_->pad = 3;
    auto* modes = actions_->emplace<StackPanel>(Orientation::Horizontal, 0.0f);
    modes->name = "PreviewModes";
    auto mode_button = [&](const wchar_t* label, const char* nm, int value, const wchar_t* tip) {
        Button* b = make_button(*modes, label, nm, [this, value] { request_mode(value); });
        b->tooltip = tip;
        return b;
    };
    auto_ = mode_button(L"Auto", "PreviewModeAuto", 0, L"Show the file the way its type suggests");
    raw_ = mode_button(L"Raw", "PreviewModeRaw", 1, L"Decode the bytes as text, whatever the file claims to be");
    hex_ = mode_button(L"Hex", "PreviewModeHex", 2, L"A hex dump: offset, bytes, ASCII");
    fit_ = make_button(*actions_, L"100%", "PreviewFit", [this] { request_fit(); });
    fit_->tooltip = L"Fit to the pane, or show at 100% (double-click the image). Scroll to zoom, drag to pan";
    wrap_ = make_button(*actions_, L"Wrap", "PreviewWrap", [this] { request_wrap(); });
    wrap_->tooltip = L"Wrap long lines (turns the line numbers off)";
    copy_ = make_button(*actions_, L"Copy", "PreviewCopy", [this] { copy_text(); });
    copy_->tooltip = L"Copy the previewed text";

    // ---- the foot: whichever footer belongs to the body showing ----
    footer_box_ = emplace<Panel>();
    footer_box_->dock = Dock::Bottom;
    footer_box_->height = 24;
    footer_box_->pad = 4;
    footer_ = footer_box_->emplace<TextBlock>();
    footer_->name = "PreviewFooter";
    footer_->style.font.size = 11.0f;
    footer_->style.fg = tok::TextMuted;
    footer_->align = gfx::TextAlign::Leading;

    // ---- the animation's transport ----
    gif_bar_ = emplace<DockPanel>();
    gif_bar_->name = "GifTransport";
    gif_bar_->automation_id = "GifTransport";
    gif_bar_->dock = Dock::Bottom;
    gif_bar_->height = 34;
    gif_bar_->pad = 3;
    gif_bar_->set_visible(false);
    gif_play_ = gif_bar_->emplace<Button>();
    gif_play_->name = "GifPlayPause";
    gif_play_->automation_id = "GifPlayPause";
    gif_play_->dock = Dock::Left;
    gif_play_->width = 32;
    gif_play_->height = 28;
    gif_play_->tooltip = L"Play or pause (Space)";
    gif_play_->icon = icons::find("Pause");
    gif_play_->on_click = [this] { animation_toggle(); };
    gif_time_ = gif_bar_->emplace<TextBlock>();
    gif_time_->name = "GifTimeText";
    gif_time_->dock = Dock::Right;
    gif_time_->width = 190;
    gif_time_->style.font.size = 11.0f;
    gif_time_->style.fg = tok::TextSecondary;
    gif_time_->align = gfx::TextAlign::Trailing;
    gif_seek_ = gif_bar_->emplace<Slider>();
    gif_seek_->name = "GifSeek";
    gif_seek_->automation_id = "GifSeek";
    gif_seek_->dock = Dock::Fill;
    gif_seek_->step = 1;
    gif_seek_->on_change = [this](double v, bool dragging) { animation_slider_changed(v, dragging); };

    // ---- the bodies; one shows at a time ----
    body_ = emplace<Panel>();
    body_->dock = Dock::Fill;
    code_ = body_->emplace<CodeView>();
    code_->name = "PreviewText";
    // What a text box offers on a right click. Spec: PreviewPane.xaml (the RichTextBox's own menu).
    code_->on_context = [this](Point at) {
        if (!on_context_menu) return;
        MenuItems items;
        MenuItem copy = MenuItem::make(L"&Copy", [this] { code_->copy_selection(); });
        copy.enabled = code_->has_selection();
        copy.icon = &icons::Copy;
        items.push_back(std::move(copy));
        items.push_back(MenuItem::make(L"Select &all", [this] { code_->select_all(); }));
        on_context_menu(std::move(items), at);
    };
    image_ = body_->emplace<ImageView>();
    image_->name = "PreviewImage";
    image_->on_toggle_playing = [this] { animation_toggle(); };
    image_->on_fit_changed = [this] { sync_chrome(); };
    image_->on_fit_request = [this] { request_fit(); };
    image_->on_left_fit = [this] { request_fit(); };  // the tab's switch follows: it reads "Fit" now
    archive_rows_ = std::make_unique<ArchiveRows>();
    archive_ = body_->emplace<ListView>();
    archive_->name = "PreviewArchive";
    archive_->set_provider(archive_rows_.get());
    {
        Column name_col;
        name_col.id = 0;
        name_col.title = L"Name";
        name_col.width = 170;
        name_col.sortable = false;
        Column size_col;
        size_col.id = 1;
        size_col.title = L"Size";
        size_col.width = 70;
        size_col.right_align = true;
        size_col.sortable = false;
        Column packed_col = size_col;
        packed_col.id = 2;
        packed_col.title = L"Packed";
        archive_->set_columns({name_col, size_col, packed_col});
    }
    font_ = body_->emplace<FontView>();
    font_->name = "PreviewFont";
    media_ = body_->emplace<MediaPane>(core_, tab_);
    media_->name = "PreviewMedia";
    // The slot the video left says where it went. Spec: PreviewPane.EnterFullScreen ("Playing full screen").
    media_->on_full_screen = [this](bool on) {
        if (on_full_screen) on_full_screen(on);
        show_body(state_.c_str());
        request_layout();
        invalidate();
    };
    media_->on_next = [this] {
        if (on_next_media) on_next_media();
    };
    message_ = body_->emplace<TextBlock>();
    message_->name = "PreviewMessage";
    message_->wrap = true;
    message_->vcenter = true;
    message_->align = gfx::TextAlign::Center;
    message_->style.fg = tok::TextMuted;
    show_body("none");
    sync_chrome();

    worker_ = std::thread([this] { worker_main(); });
}

PreviewPane::~PreviewPane() {
    {
        std::lock_guard<std::mutex> lock(mu_);
        stop_ = true;
    }
    cv_.notify_all();
    if (worker_.joinable()) worker_.join();
    animation_stop();
    if (media_) media_->stop();  // full screen lends it to the window; it comes home before the pane goes
}

uint64_t PreviewPane::now() const { return root() ? root()->now_ms() : GetTickCount64(); }

// ---- scheduling ------------------------------------------------------------------------------------------

std::wstring PreviewPane::selection_signature() {
    const std::vector<uint32_t> sel = core_.selection(tab_);
    if (sel.size() != 1) return L"#" + std::to_wstring(sel.size());
    auto listing = core_.listing(tab_);
    BbRow row{};
    if (!listing || !listing->row(sel[0], row)) return L"";
    return core::w(row.full_path) + L"|" + std::to_wstring(row.modified) + L"|" + std::to_wstring(row.size);
}

void PreviewPane::show() {
    if (!active_) return;
    const std::wstring sig = selection_signature();
    if (sig == shown_signature_ && mode_ == shown_mode_ && sig[0] != L'#') return;
    shown_signature_ = sig;
    shown_mode_ = mode_;
    schedule(true);
}

void PreviewPane::reload() {
    shown_signature_ = selection_signature();
    shown_mode_ = mode_;
    schedule(false);
}

void PreviewPane::schedule(bool debounce) {
    ++scheduled_;
    latest_ = scheduled_;
    loading_ = true;
    {
        std::lock_guard<std::mutex> lock(mu_);
        request_ = Request{scheduled_, mode_, debounce, true};
    }
    cv_.notify_all();
}

void PreviewPane::run(const char* command) { bb_command_run(core_.app(), core::s(core::wide(command))); }

void PreviewPane::set_mode(int mode) {
    if (mode_ == mode) return;
    mode_ = mode;
    // A mode button is one deliberate gesture: no debounce, which is for a sweep of selections.
    if (active_) {
        shown_mode_ = mode_;
        shown_signature_ = selection_signature();
        schedule(false);
    } else {
        shown_mode_ = -1;
    }
    sync_chrome();
}

void PreviewPane::apply_view_state(int mode, bool wrap, bool actual_size) {
    if (code_->wrap() != wrap) code_->set_wrap(wrap);
    if (image_->fitted() == actual_size) image_->set_fitted(!actual_size);
    set_mode(mode);
    sync_chrome();
}

void PreviewPane::copy_text() {
    if (!root() || !root()->clipboard.write) return;
    const std::wstring text = code_->has_selection() ? code_->selected_text() : code_->all_text();
    if (!text.empty()) root()->clipboard.write(text);
}

// ---- the worker ------------------------------------------------------------------------------------------

void PreviewPane::worker_main() {
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    for (;;) {
        Request r;
        {
            std::unique_lock<std::mutex> lock(mu_);
            cv_.wait(lock, [this] { return stop_ || request_.valid; });
            if (stop_) break;
            r = request_;
            request_.valid = false;
        }
        if (r.debounce) {
            // Long enough to swallow an arrow-key sweep down a list, short enough that a click feels immediate.
            // A newer request (schedule notifies) ends the wait early. Sleep(10) in a loop rounds each step
            // up to the 15.6 ms timer tick, which made the 150 ms debounce 234 ms.
            std::unique_lock<std::mutex> lock(mu_);
            cv_.wait_for(lock, std::chrono::milliseconds(150), [this] { return stop_ || request_.valid; });
        }
        if (latest_ != r.generation || stop_) continue;
        {
            // The debounce is over: what was on show is for another selection, so the pane says it is loading.
            auto started = std::make_unique<PreviewResult>();
            started->generation = r.generation;
            started->tab = tab_;
            started->mode = r.mode;
            started->started = true;
            if (PostMessageW(window_, WM_BB_PREVIEW, static_cast<WPARAM>(tab_), reinterpret_cast<LPARAM>(started.get()))) started.release();
        }
        std::unique_ptr<PreviewResult> result;
        try {
            result = build(r);
        } catch (const std::exception&) {
            result.reset();
        }
        if (!result) {
            result = std::make_unique<PreviewResult>();
            result->generation = r.generation;
            result->tab = tab_;
            result->mode = r.mode;
        }
        if (latest_ != r.generation || stop_) {
            if (result->gif) bb_gif_close(core_.app(), static_cast<uint64_t>(result->gif));
            continue;
        }
        if (!PostMessageW(window_, WM_BB_PREVIEW, static_cast<WPARAM>(tab_), reinterpret_cast<LPARAM>(result.get()))) {
            if (result->gif) bb_gif_close(core_.app(), static_cast<uint64_t>(result->gif));
            continue;
        }
        result.release();
    }
}

std::unique_ptr<PreviewResult> PreviewPane::build(const Request& r) {
    auto out = std::make_unique<PreviewResult>();
    out->generation = r.generation;
    out->tab = tab_;
    out->mode = r.mode;

    auto ask = [&](int mode) -> json::Value {
        BbOwnedStr o{};
        if (bb_preview_build(core_.app(), tab_, static_cast<uint32_t>(mode), &o) != BB_OK) {
            return json::parse("{\"state\":\"none\",\"message\":\"This file could not be read.\"}");
        }
        return json::parse(core::utf8(core::take(o)));
    };
    out->model = ask(r.mode);
    auto& m = out->model;
    const std::string state = m["state"].str();
    const std::wstring path = core::wide(m["path"].str());

    auto set_message = [&](const char* text) {
        for (auto& kv : m.o) {
            if (kv.first == "state") kv.second.s = "none";
            if (kv.first == "message") {
                kv.second.type = json::Value::Type::String;
                kv.second.s = text;
            }
        }
    };

    if (state == "image") {
        if (ends_with_nocase(path, L".gif")) {
            BbOwnedStr o{};
            uint64_t handle = 0;
            if (bb_gif_open(core_.app(), core::s(path), &handle, &o) == BB_OK) {
                const json::Value info = json::parse(core::utf8(core::take(o)));
                if (info["animated"].boolean() && handle) {
                    const uint32_t w = static_cast<uint32_t>(info["width"].i64()), h = static_cast<uint32_t>(info["height"].i64());
                    auto frame = std::make_shared<ui::ThumbImage>();
                    frame->width = w;
                    frame->height = h;
                    frame->bgra.resize(static_cast<size_t>(w) * h * 4);
                    uint32_t at = 0;
                    if (bb_gif_frame(core_.app(), handle, 0, frame->bgra.data(), frame->bgra.size(), &at) == BB_OK) {
                        out->image = frame;
                        out->image_w = w;
                        out->image_h = h;
                        out->gif = static_cast<int64_t>(handle);
                        out->gif_frames = static_cast<int>(info["frames"].i64());
                        out->gif_duration = info["durationMs"].i64();
                    } else {
                        bb_gif_close(core_.app(), handle);
                    }
                } else if (info["reason"].str() == "tooLarge") {
                    out->image_footer = core::utf8(thousands(info["frames"].i64()) + L" frames  ·  too large to animate here");
                }
            }
        }
        if (!out->image && m["inArchive"].boolean()) {
            // A picture inside an archive is decoded from the entry's bytes; the shell has nothing to say about a
            // path that is not on disk, so there is no second try.
            std::vector<uint8_t> bytes(static_cast<size_t>(std::max<int64_t>(m["sizeBytes"].i64(), 0)) + 1);
            size_t len = 0;
            BbStatus st = bb_preview_read(core_.app(), core::s(path), bytes.data(), bytes.size(), &len);
            if (st == BB_OK && len > bytes.size()) {
                bytes.resize(len);
                st = bb_preview_read(core_.app(), core::s(path), bytes.data(), bytes.size(), &len);
            }
            if (st != BB_OK) {
                const std::string why = core::utf8(core::last_error());
                set_message(why.empty() ? "This file could not be read." : why.c_str());
                return out;
            }
            bytes.resize(len);
            out->image = decode_picture(bytes, 2048, &out->image_w, &out->image_h);
            if (!out->image) set_message("No preview available");
            return out;
        }
        if (!out->image) out->image = decode_picture_file(path, 2048, &out->image_w, &out->image_h);
        if (!out->image) {
            // No codec for this format (HEIC and camera raw, most often): the shell may still have a handler.
            out->image = cached_shell_preview(path, 1024);
            if (out->image) {
                out->image_w = out->image->width;
                out->image_h = out->image->height;
            }
        }
        if (!out->image) set_message("No preview available");
    } else if (state == "document") {
        out->image = cached_shell_preview(path, 1024);
        if (out->image) {
            out->image_w = 0;
        } else {
            // The shell declined: the bytes get a look before giving up. Explorer stops here, and it is the
            // most common way its pane is useless.
            out->model = ask(3);
        }
    } else if (state == "media") {
        out->image = cached_shell_preview(path, 1024);  // the poster, if the shell has one
    } else if (state == "font") {
        out->font = read_font_info(path);
        if (!out->font.ok) set_message("This font could not be read.");
    }
    return out;
}

// ---- applying an answer ----------------------------------------------------------------------------------

void PreviewPane::apply(std::unique_ptr<PreviewResult> r) {
    if (r->generation < scheduled_) {
        // A newer request is already on its way; this one is for a selection that has moved on.
        if (r->gif) bb_gif_close(core_.app(), static_cast<uint64_t>(r->gif));
        return;
    }
    if (r->started) {
        show_loading();
        return;
    }
    applied_ = r->generation;
    loading_ = false;
    animation_stop();
    media_->stop(r->model["state"].str() == "media");
    // A font held in a private collection holds its file; nothing here holds a file open past the preview.
    font_->set_font(L"");

    const json::Value& m = r->model;
    state_ = m["state"].str("none");
    title_text_ = core::wide(m["title"].str());
    base_subtitle_ = core::wide(m["subtitle"].str());
    subtitle_text_ = base_subtitle_;
    message_text_ = m["message"].is_null() ? L"" : core::wide(m["message"].str());
    can_choose_mode_ = m["canChooseMode"].boolean();
    footer_text_.clear();
    image_footer_text_ = core::wide(r->image_footer);
    const std::wstring path = core::wide(m["path"].str());

    if (state_ == "text" || state_ == "hex") {
        set_text_content(m, state_ == "hex");
        footer_text_ = core::wide(m["footer"].str());
    } else if (state_ == "image" || state_ == "document") {
        image_->set_image(r->image);
        if (r->image_w > 0) subtitle_text_ = join({std::to_wstring(r->image_w) + L" × " + std::to_wstring(r->image_h), base_subtitle_});
        footer_text_ = image_footer_text_;
        if (r->gif) animation_start(*r);
    } else if (state_ == "archive") {
        archive_rows_->rows.clear();
        for (size_t i = 0; i < m["entries"].size(); ++i) {
            const json::Value& e = m["entries"][i];
            archive_rows_->rows.push_back({core::wide(e["path"].str()),
                                           e["directory"].boolean() ? L"" : byte_size(e["size"].i64()),
                                           e["directory"].boolean() ? L"" : byte_size(e["packed"].i64())});
        }
        archive_->refresh();
        footer_text_ = core::wide(m["footer"].str());
    } else if (state_ == "font") {
        font_->set_font(path);
        footer_text_ = join({r->font.family, r->font.face, thousands(r->font.glyphs) + L" glyphs", r->font.version});
    } else if (state_ == "media") {
        media_->open(path, r->image, m["isVideo"].boolean());
        if (!play_when_loaded_.empty() && _wcsicmp(play_when_loaded_.c_str(), path.c_str()) == 0) media_->play();
    }
    play_when_loaded_.clear();
    show_body(state_.c_str());
    sync_chrome();
    request_layout();
    invalidate();
}

// Spec: PreviewPaneViewModel.LoadAsync (Describe, then IsLoading with every body hidden). A selection that needs
// no disk (nothing, several, a folder) answers at once and never says it is loading.
void PreviewPane::show_loading() {
    const std::vector<uint32_t> sel = core_.selection(tab_);
    if (sel.size() != 1) return;
    auto listing = core_.listing(tab_);
    BbRow row{};
    if (!listing || !listing->row(sel[0], row) || (row.attributes & FILE_ATTRIBUTE_DIRECTORY)) return;
    animation_stop();
    media_->stop(true);  // full screen waits for what arrives: the next video keeps it, anything else ends it
    font_->set_font(L"");
    state_ = "none";
    title_text_ = core::w(row.name);
    base_subtitle_.clear();
    subtitle_text_.clear();
    message_text_.clear();
    footer_text_.clear();
    image_->set_image(nullptr);
    show_body("none");
    if (media_->full_screen()) media_->set_visible(true);
    sync_chrome();
    request_layout();
    invalidate();
}

void PreviewPane::set_text_content(const json::Value& m, bool hex) {
    std::vector<std::wstring> lines;
    const json::Value& jl = m["lines"];
    lines.reserve(jl.size());
    for (size_t i = 0; i < jl.size(); ++i) lines.push_back(core::wide(jl[i].str()));
    std::vector<CodeView::Span> spans;
    const json::Value& js = m["spans"];
    spans.reserve(js.size());
    for (size_t i = 0; i < js.size(); ++i) {
        const json::Value& s = js[i];
        spans.push_back({static_cast<uint32_t>(s[static_cast<size_t>(0)].i64()), static_cast<uint32_t>(s[static_cast<size_t>(1)].i64()),
                         static_cast<uint32_t>(s[static_cast<size_t>(2)].i64()), static_cast<uint8_t>(s[static_cast<size_t>(3)].i64())});
    }
    // A hex row already carries its offset, and that offset is the gutter: a decimal row number beside it
    // would be a second, worse one.
    code_->set_content(std::move(lines), std::move(spans), !hex);
}

void PreviewPane::show_body(const char* which) {
    const std::string w = which;
    const bool text = w == "text" || w == "hex";
    const bool image = w == "image" || w == "document";
    code_->set_visible(text);
    image_->set_visible(image);
    archive_->set_visible(w == "archive");
    font_->set_visible(w == "font");
    media_->set_visible(w == "media");
    message_->set_visible(!text && !image && w != "archive" && w != "font" && w != "media");
    message_->set_text(message_text_.empty() && loading_ ? L"Loading…" : message_text_);
    if (w == "media" && !message_text_.empty()) message_->set_visible(false);
    if (media_->full_screen()) {
        message_->set_text(L"Playing full screen");
        message_->set_visible(true);
    }
}

void PreviewPane::sync_chrome() {
    title_->set_text(title_text_);
    title_->tooltip = title_text_;  // a long name is trimmed in the header
    subtitle_->set_text(subtitle_text_);
    const bool text = state_ == "text", hex = state_ == "hex";
    const bool image = state_ == "image" || state_ == "document";
    const bool choose = can_choose_mode_ && !loading_;
    actions_->children()[0]->set_visible(choose);
    auto_->primary = mode_ == 0;
    raw_->primary = mode_ == 1;
    hex_->primary = mode_ == 2;
    fit_->set_visible(image);
    fit_->text = image_->fitted() ? L"100%" : L"Fit";
    wrap_->set_visible(text);
    wrap_->primary = code_->wrap();
    copy_->set_visible(text || hex);
    actions_->set_visible(choose || image || text || hex);
    footer_->set_text(footer_text_);
    footer_box_->set_visible(!footer_text_.empty());
    gif_bar_->set_visible(gif_ != 0);
    set_playing_icon();
    request_layout();
    invalidate();
}

std::string PreviewPane::state_name() const {
    if (loading_) return "loading";
    if (state_ == "image" || state_ == "document" || state_ == "text" || state_ == "hex" || state_ == "archive" || state_ == "font" ||
        state_ == "media") {
        return state_;
    }
    return "none";
}

float PreviewPane::desired_width() {
    if (state_ == "text" || state_ == "hex") return code_->desired_width();
    if (state_ == "image" || state_ == "document") return image_->natural_width();
    return bounds.width();
}

void PreviewPane::set_active(bool active) {
    if (active_ == active) return;
    active_ = active;
    if (!active) {
        animation_pause_clock();
        media_->suspend();
        return;
    }
    animation_resume_clock();
    show();
}

// ---- the animation ---------------------------------------------------------------------------------------
//
// Plays an animated GIF by swapping the image's frame in place, so zoom, pan and fit apply to it as to a
// still. The frame shown is always read off a clock (an offset plus the time since it last started), never
// counted by ticks, so a late tick skips ahead rather than slowing the animation down. The timer is re-armed
// each tick for exactly the time left on the current frame, and stopped whenever nothing would see it.

int64_t PreviewPane::animation_position() const {
    return offset_ms_ + (clock_running_ ? static_cast<int64_t>(now() - clock_start_) : 0);
}

void PreviewPane::animation_start(const PreviewResult& r) {
    gif_ = r.gif;
    gif_frames_ = r.gif_frames;
    gif_duration_ = std::max<int64_t>(1, r.gif_duration);
    shown_frame_ = -1;
    offset_ms_ = 0;
    clock_running_ = false;
    scrubbing_ = false;
    // The last millisecond of the loop, not its length: the duration itself wraps back to frame 0, and a
    // thumb dragged to the very end should hold on the last frame.
    gif_seek_->min = 0;
    gif_seek_->max = static_cast<double>(gif_duration_ - 1);
    gif_seek_->step = 1;
    gif_seek_->large_step = std::max<double>(1.0, static_cast<double>(gif_duration_) / 10.0);
    gif_seek_->set_value(0);
    playing_ = true;
    animation_show_at(0);
    animation_resume_clock();
}

void PreviewPane::animation_stop() {
    animation_pause_clock();
    if (root()) root()->stop_timer(this, kAnimTimer);
    if (gif_) bb_gif_close(core_.app(), static_cast<uint64_t>(gif_));
    gif_ = 0;
    gif_frames_ = 0;
    shown_frame_ = -1;
    playing_ = false;
    offset_ms_ = 0;
    gif_time_->set_text(L"");
    gif_bar_->set_visible(false);
}

void PreviewPane::animation_resume_clock() {
    if (!gif_ || !playing_ || scrubbing_ || !active_) return;
    if (clock_running_) return;
    clock_start_ = now();
    clock_running_ = true;
    animation_arm();
}

void PreviewPane::animation_pause_clock() {
    if (root()) root()->stop_timer(this, kAnimTimer);
    if (!clock_running_) return;
    offset_ms_ = animation_position();
    clock_running_ = false;
}

void PreviewPane::animation_arm() {
    if (!gif_ || !root()) return;
    uint32_t frame = 0;
    int64_t until = 1;
    bb_gif_position(core_.app(), static_cast<uint64_t>(gif_), animation_position(), &frame, &until);
    root()->start_timer(this, kAnimTimer, static_cast<unsigned>(std::max<int64_t>(1, until)));
}

void PreviewPane::on_timer(int id) {
    if (id == kAnimTimer) animation_tick();
}

void PreviewPane::animation_tick() {
    if (!gif_) {
        root()->stop_timer(this, kAnimTimer);
        return;
    }
    const uint64_t handle = static_cast<uint64_t>(gif_);
    if (!clock_running_) {
        // Held on a frame that had not decoded yet: fill it in once it lands, then stop.
        root()->stop_timer(this, kAnimTimer);
        animation_show_at(offset_ms_);
        return;
    }
    // Folded back into one pass now and then, so the offset never grows without bound.
    if (offset_ms_ > gif_duration_ * 1000) offset_ms_ %= gif_duration_;

    const int64_t position = animation_position();
    uint32_t frame = 0, ready = 0, ready_flags = 0;
    int64_t until = 1;
    bb_gif_position(core_.app(), handle, position, &frame, &until);
    bb_gif_ready(core_.app(), handle, &ready, &ready_flags);
    const bool stopped = (ready_flags & 2) != 0;
    if (frame >= ready) {
        // The clock has caught up with the decoder. If nothing more is coming, loop over what did decode;
        // otherwise hold at the start of the missing frame until it lands: buffering, as a video would,
        // rather than skipping frames nobody has seen.
        int64_t start = 0;
        bb_gif_start_of(core_.app(), handle, frame, &start);
        offset_ms_ = stopped ? 0 : start;
        clock_start_ = now();
        animation_show_at(offset_ms_);
        if (!stopped) {
            root()->start_timer(this, kAnimTimer, 15);
            return;
        }
    } else {
        animation_show_at(position);
    }
    animation_arm();
}

std::wstring PreviewPane::clock_text(int64_t ms) {
    wchar_t buf[48];
    const int64_t tenths = ms / 100;
    if (ms >= 60000) swprintf(buf, 48, L"%lld:%02lld.%lld", static_cast<long long>(ms / 60000), static_cast<long long>(ms / 1000 % 60), static_cast<long long>(tenths % 10));
    else swprintf(buf, 48, L"%lld.%lld", static_cast<long long>(ms / 1000), static_cast<long long>(tenths % 10));
    return buf;
}

void PreviewPane::animation_show_at(int64_t position_ms) {
    if (!gif_) return;
    const uint64_t handle = static_cast<uint64_t>(gif_);
    const int64_t position = ((position_ms % gif_duration_) + gif_duration_) % gif_duration_;
    uint32_t wanted = 0, ready = 0, ready_flags = 0;
    int64_t until = 1;
    bb_gif_position(core_.app(), handle, position, &wanted, &until);
    bb_gif_ready(core_.app(), handle, &ready, &ready_flags);
    const bool complete = (ready_flags & 1) != 0, stopped = (ready_flags & 2) != 0;
    // A frame not decoded yet shows the last one that is; a tick fills it in once it lands, while playing the
    // ordinary tick, and while held it is armed here.
    const int frame = static_cast<int>(std::min<uint32_t>(wanted, ready ? ready - 1 : 0));
    if (static_cast<int>(wanted) > frame && !stopped && !clock_running_) root()->start_timer(this, kAnimTimer, 30);

    if (frame != shown_frame_) {
        shown_frame_ = frame;
        const ui::ThumbPtr& current = image_->image();
        if (current) {
            auto pixels = std::make_shared<ui::ThumbImage>();
            pixels->width = current->width;
            pixels->height = current->height;
            pixels->bgra.resize(current->bgra.size());
            uint32_t at = 0;
            if (bb_gif_frame(core_.app(), handle, static_cast<uint32_t>(frame), pixels->bgra.data(), pixels->bgra.size(), &at) == BB_OK) {
                image_->set_frame(pixels);
            }
        }
    }
    if (!scrubbing_) gif_seek_->set_value(static_cast<double>(position));

    std::wstring text = std::to_wstring(frame + 1) + L" / " + std::to_wstring(gif_frames_) + L"  ·  " + clock_text(position) + L" / " +
                        clock_text(gif_duration_);
    if (!complete && !stopped) text += L"  ·  loading " + std::to_wstring(static_cast<int64_t>(ready) * 100 / std::max(1, gif_frames_)) + L"%";
    gif_time_->set_text(text);
}

void PreviewPane::set_playing_icon() { gif_play_->icon = icons::find(playing_ ? "Pause" : "Play"); }

void PreviewPane::animation_toggle() {
    if (!gif_) return;
    if (playing_) {
        animation_pause_clock();
        playing_ = false;
    } else {
        playing_ = true;
        animation_resume_clock();
    }
    set_playing_icon();
    gif_play_->invalidate();
}

void PreviewPane::animation_set_playing(bool want) {
    if (!gif_ || clock_running_ == want) return;
    animation_toggle();
}

void PreviewPane::animation_seek(double ms) {
    if (!gif_) return;
    gif_seek_->set_value(ms);
    animation_slider_changed(gif_seek_->value(), false);
}

// Every move of the seek bar that is not the tick's is the user's: a scrub, an arrow key. Each shows its
// frame at once; a frame is a lookup, so unlike a video's seek bar this needs no throttle.
void PreviewPane::animation_slider_changed(double value, bool dragging) {
    if (!gif_) return;
    if (dragging && !scrubbing_) {
        resume_after_scrub_ = playing_;
        animation_pause_clock();
        scrubbing_ = true;
    }
    if (scrubbing_) {
        offset_ms_ = static_cast<int64_t>(value);
        animation_show_at(offset_ms_);
        if (!dragging) {
            scrubbing_ = false;
            if (resume_after_scrub_) animation_resume_clock();
        }
        return;
    }
    const bool running = clock_running_;
    animation_pause_clock();
    offset_ms_ = static_cast<int64_t>(value);
    animation_show_at(offset_ms_);
    if (running) animation_resume_clock();
}

}  // namespace bb::views
