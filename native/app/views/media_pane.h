// The media body of the preview: a poster frame, and nothing else until someone presses play. Then the
// engine opens the file (it is held open from then until the selection moves), frames arrive on a timer, and
// a transport appears: a seek bar that follows the mouse as it is dragged, play and pause, the next video in
// the folder, the clock, three switches (play on arrival, loop, go on to the next), full screen and volume.
//
// Full screen lends this whole widget to the window: it is taken out of the pane and laid over everything,
// so the one engine keeps playing without a reopen and every control keeps its handler. Esc, or the
// button again, puts it back.
#pragma once
#include "ui/controls.h"
#include "ui/inputs.h"
#include "ui/layout.h"
#include "views/core.h"
#include "views/image_view.h"
#include "views/media_player.h"

#include <functional>
#include <memory>

namespace bb::views {

class MediaPane : public ui::DockPanel {
public:
    MediaPane(core::Core& core, uint64_t tab);
    ~MediaPane() override;

    // The file the pane is on, and its poster (null when the shell has none). Nothing is opened.
    void open(const std::wstring& path, ui::ThumbPtr poster, bool is_video);
    void play();                 // opens the file and starts it
    void toggle_play();
    void stop();                 // closes the engine and goes back to the poster
    void suspend();              // the pane was hidden: pause, keep the place
    bool started() const { return started_; }
    bool playing() const { return player_ && player_->playing(); }
    bool is_video() const { return is_video_; }
    void set_full_screen(bool on);
    bool full_screen() const { return full_; }
    std::function<void()> on_next;  // ask the tab to select the next video

    ui::Slider* seek() const { return seek_; }
    ui::Slider* volume() const { return volume_; }
    ui::Button* play_button() const { return play_; }

    void arrange(ui::Rect r) override;
    void on_timer(int id) override;
    bool on_key_down(const ui::KeyEvent& e) override;

private:
    void tick();
    void start_ticking();
    void sync_transport();
    void toggle_setting(const char* key, const char* other);
    bool setting(const char* key) const;
    // The three switches are read from the core when they could have moved, not on every tick or in a destructor.
    void load_settings();
    void seek_changed(double value, bool dragging);
    static std::wstring clock_text(double seconds);

    core::Core& core_;
    uint64_t tab_;
    std::unique_ptr<MediaPlayer> player_;
    std::wstring path_;
    bool is_video_ = false, started_ = false, full_ = false, scrubbing_ = false, resume_after_scrub_ = false;
    bool ended_handled_ = false;

    ImageView* screen_ = nullptr;
    ui::Button *big_play_ = nullptr, *play_ = nullptr, *next_ = nullptr, *autoplay_ = nullptr, *loop_ = nullptr, *advance_ = nullptr,
               *fullscreen_ = nullptr;
    ui::TextBlock* time_ = nullptr;
    ui::Slider *seek_ = nullptr, *volume_ = nullptr;
    ui::StackPanel* controls_ = nullptr;
    ui::DockPanel* transport_ = nullptr;
    ui::Widget* home_ = nullptr;  // the widget it came from, to go back to
    uint64_t last_seek_ms_ = 0;
    bool autoplay_on_ = false, loop_on_ = false, advance_on_ = false;
};

}  // namespace bb::views
