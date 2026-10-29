// The preview pane of one tab: the selection turned into something to look at. A text file with syntax
// colours, a hex dump, a picture (a GIF plays, with a seek bar), the listing of an archive, a font specimen,
// a video's poster, a page of a document; or, never blank, the reason there is nothing.
//
// Four rules shape this, and each is a thing Explorer's pane gets wrong.
//  - **No file is held open.** Every read happens on a worker, into memory, handle closed before a pixel is
//    decoded, so previewing never blocks renaming, moving or deleting what is being previewed.
//  - **Nothing blocks the UI thread.** One worker thread, cancel-previous: a newer request makes an older
//    answer drop on the floor, and a debounce means a sweep down two hundred rows costs one preview.
//  - **Selection churn is free.** A selection that has not changed (same paths, same modified times, same
//    mode) starts nothing.
//  - **A cloud placeholder is never hydrated.** The core's classifier refuses it before a byte is read.
//
// The pane is built the first time it is shown, not with the tab.
#pragma once
#include "ui/controls.h"
#include "ui/inputs.h"
#include "ui/layout.h"
#include "ui/list_view.h"
#include "views/code_view.h"
#include "views/core.h"
#include "views/font_view.h"
#include "views/image_view.h"
#include "views/json.h"
#include "views/pane_images.h"

#include <atomic>
#include <condition_variable>
#include <memory>
#include <mutex>
#include <thread>

namespace bb::views {

constexpr UINT WM_BB_PREVIEW = WM_APP + 4;

class MediaPane;

// What the worker hands the UI thread: the core's answer and whatever only a worker could fetch.
struct PreviewResult {
    uint64_t generation = 0;
    uint64_t tab = 0;
    int mode = 0;
    json::Value model;
    ui::ThumbPtr image;  // a picture, a document's page, a video's poster
    uint32_t image_w = 0, image_h = 0;
    std::string image_footer;
    int64_t gif = 0;  // handle of the animation playing, or 0
    int gif_frames = 0;
    int64_t gif_duration = 0;
    FontInfo font;
};

class PreviewPane : public ui::DockPanel {
public:
    // The things the core says about a GIF frame count that the pane words.
    static constexpr int kAnimTimer = 1;

    PreviewPane(core::Core& core, uint64_t tab, HWND window);
    ~PreviewPane() override;

    // ---- driving it ----
    // The selection, or the pane's visibility, may have moved: loads if the selection is not what is shown.
    void show();
    // Whether anyone can see it. Hidden, the animation clock stops and the media pauses; shown again,
    // only what was playing resumes.
    void set_active(bool active);
    bool active() const { return active_; }
    // The core owns the mode, the wrap and the 100% switch (a key and the palette change them too): a button
    // asks it, and the tab hands the answer back through apply_view_state when it changes.
    void request_mode(int mode) { run(mode == 1 ? "preview.mode-raw" : mode == 2 ? "preview.mode-hex" : "preview.mode-auto"); }
    void request_wrap() { run("preview.wrap"); }
    void request_fit() { run("preview.fit"); }
    void apply_view_state(int mode, bool wrap, bool actual_size);
    int mode() const { return mode_; }
    void reload();
    void copy_text();
    // The file that plays as soon as it has loaded: set by the tab as it steps on to the next video.
    void play_when_loaded(const std::wstring& path) { play_when_loaded_ = path; }
    // The UI thread, when WM_BB_PREVIEW lands.
    void apply(std::unique_ptr<PreviewResult> result);
    // A request is scheduled or running (the harness's settle-preview waits for none).
    bool pending() const { return scheduled_ != applied_; }

    // ---- reading it back ----
    // loading | none | text | hex | image | document | archive | font | media
    std::string state_name() const;
    const std::wstring& title() const { return title_text_; }
    const std::wstring& subtitle() const { return subtitle_text_; }
    const std::wstring& message() const { return message_text_; }
    const std::wstring& footer() const { return footer_text_; }
    CodeView* code() const { return code_; }
    ImageView* image() const { return image_; }
    bool can_choose_mode() const { return can_choose_mode_; }
    // The width the pane would need to show what it holds with no horizontal bar.
    float desired_width();
    std::function<void()> on_fit_width;  // the tab widens the pane
    // Selects what plays next in the list (media); the tab does it, the pane only knows its file.
    std::function<void()> on_next_media;

    // ---- the animation, for the harness and the transport ----
    bool animated() const { return gif_ != 0; }
    bool animation_running() const { return clock_running_; }
    int animation_frame() const { return gif_ ? shown_frame_ : -1; }
    void animation_toggle();
    void animation_seek(double ms);  // as the seek bar's thumb moved to it
    void animation_set_playing(bool playing);
    ui::Slider* gif_seek() const { return gif_seek_; }
    ui::Button* gif_play_pause() const { return gif_play_; }

    // ---- media ----
    MediaPane* media() const { return media_; }

    void on_timer(int id) override;

private:
    struct Request {
        uint64_t generation = 0;
        int mode = 0;
        bool debounce = true;
        bool valid = false;
    };

    std::wstring selection_signature();
    void run(const char* command);
    void set_mode(int mode);
    void schedule(bool debounce);
    void worker_main();
    std::unique_ptr<PreviewResult> build(const Request& r);
    void show_body(const char* which);
    void sync_chrome();
    void set_text_content(const json::Value& model, bool hex);

    // the animation
    void animation_start(const PreviewResult& r);
    void animation_stop();
    void animation_resume_clock();
    void animation_pause_clock();
    void animation_arm();
    void animation_tick();
    void animation_show_at(int64_t position_ms);
    uint64_t now() const;
    int64_t animation_position() const;
    void animation_slider_changed(double value, bool dragging);
    void set_playing_icon();
    static std::wstring clock_text(int64_t ms);

    core::Core& core_;
    uint64_t tab_;
    HWND window_;

    // chrome
    ui::TextBlock *title_ = nullptr, *subtitle_ = nullptr, *footer_ = nullptr, *gif_time_ = nullptr, *message_ = nullptr;
    ui::Button *fit_width_ = nullptr, *auto_ = nullptr, *raw_ = nullptr, *hex_ = nullptr, *fit_ = nullptr, *wrap_ = nullptr,
               *copy_ = nullptr, *gif_play_ = nullptr;
    ui::StackPanel* actions_ = nullptr;
    ui::Panel* footer_box_ = nullptr;
    ui::Panel* mode_box_ = nullptr;
    ui::DockPanel* gif_bar_ = nullptr;
    ui::Slider* gif_seek_ = nullptr;
    ui::Panel* body_ = nullptr;
    CodeView* code_ = nullptr;
    ImageView* image_ = nullptr;
    ui::ListView* archive_ = nullptr;
    FontView* font_ = nullptr;
    MediaPane* media_ = nullptr;
    struct ArchiveRows;
    std::unique_ptr<ArchiveRows> archive_rows_;

    // what is shown
    std::string state_ = "none";
    std::wstring title_text_, subtitle_text_, message_text_, footer_text_, image_footer_text_;
    std::wstring base_subtitle_;
    std::wstring play_when_loaded_;
    bool can_choose_mode_ = false;
    bool has_image_footer_ = false;
    int mode_ = 0;
    bool active_ = false;
    std::wstring shown_signature_;
    int shown_mode_ = -1;
    bool loading_ = false;

    // the worker
    std::thread worker_;
    std::mutex mu_;
    std::condition_variable cv_;
    Request request_;
    bool stop_ = false;
    std::atomic<uint64_t> latest_{0};
    uint64_t scheduled_ = 0;  // UI thread
    uint64_t applied_ = 0;    // UI thread

    // the animation
    int64_t gif_ = 0;
    int gif_frames_ = 0;
    int64_t gif_duration_ = 0;
    int shown_frame_ = -1;
    bool playing_ = false;        // what the user asked for
    bool clock_running_ = false;  // and whether the clock is actually going
    int64_t offset_ms_ = 0;
    uint64_t clock_start_ = 0;
    bool scrubbing_ = false, resume_after_scrub_ = false;
    std::vector<uint8_t> frame_buffer_;
};

}  // namespace bb::views
