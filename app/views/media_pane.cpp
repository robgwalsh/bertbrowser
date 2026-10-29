#include "views/media_pane.h"

#include "gfx/icons.gen.h"

#include <windows.h>

#include <algorithm>
#include <cmath>

namespace bb::views {

using namespace bb::ui;

namespace {

Button* tool_button(Widget& parent, const char* icon, const char* name, const wchar_t* tip, std::function<void()> click) {
    auto* b = parent.emplace<Button>();
    b->name = name;
    b->automation_id = name;
    b->icon = icons::find(icon);
    b->tooltip = tip;
    b->width = 30;
    b->height = 28;
    b->on_click = std::move(click);
    return b;
}

}  // namespace

MediaPane::MediaPane(core::Core& core, uint64_t tab) : core_(core), tab_(tab) {
    style.bg = tok::SurfaceBackground;
    flags.focusable = true;

    // ---- the transport, along the bottom; hidden until the first press of play ----
    transport_ = emplace<DockPanel>();
    transport_->name = "MediaTransport";
    transport_->dock = Dock::Bottom;
    transport_->height = 70;
    transport_->pad = 4;
    transport_->set_visible(false);
    seek_ = transport_->emplace<Slider>();
    seek_->name = "MediaSeek";
    seek_->automation_id = "MediaSeek";
    seek_->dock = Dock::Top;
    seek_->height = 24;
    seek_->min = 0;
    seek_->max = 1000;
    seek_->step = 1;
    seek_->large_step = 50;
    seek_->on_change = [this](double v, bool dragging) { seek_changed(v, dragging); };
    controls_ = transport_->emplace<StackPanel>(Orientation::Horizontal, 2.0f);
    controls_->dock = Dock::Fill;
    play_ = tool_button(*controls_, "Pause", "MediaPlayPause", L"Play or pause (Space in full screen)", [this] { toggle_play(); });
    next_ = tool_button(*controls_, "SkipNext", "MediaSkipNext", L"Next video in this folder", [this] {
        if (on_next) on_next();
    });
    time_ = controls_->emplace<TextBlock>();
    time_->name = "MediaTime";
    time_->flex = 1;
    time_->style.font.size = 11.0f;
    time_->style.fg = tok::TextSecondary;
    autoplay_ = tool_button(*controls_, "AutoPlay", "MediaAutoPlay", L"Start videos as soon as they are selected", [this] { toggle_setting("previewAutoPlay", nullptr); });
    loop_ = tool_button(*controls_, "LoopOne", "MediaLoop", L"Replay this video when it ends", [this] { toggle_setting("previewLoop", "previewAutoAdvance"); });
    advance_ = tool_button(*controls_, "AutoAdvance", "MediaAutoAdvance", L"Play the next video when this one ends", [this] { toggle_setting("previewAutoAdvance", "previewLoop"); });
    fullscreen_ = tool_button(*controls_, "FullScreen", "MediaFullScreen", L"Full screen (double-click the video; Esc to leave)", [this] { set_full_screen(!full_); });
    volume_ = controls_->emplace<Slider>();
    volume_->name = "MediaVolume";
    volume_->automation_id = "MediaVolume";
    volume_->width = 70;
    volume_->height = 28;
    volume_->min = 0;
    volume_->max = 1;
    volume_->step = 0.05;
    volume_->large_step = 0.1;
    volume_->set_value(1.0);
    volume_->tooltip = L"Volume";
    volume_->on_change = [this](double v, bool) {
        if (player_) player_->set_volume(v);
    };

    // ---- the picture: the poster until playing, then the frames ----
    screen_ = emplace<ImageView>();
    screen_->name = "MediaScreen";
    screen_->interactive = false;
    big_play_ = emplace<Button>(L"▶  Play");
    big_play_->name = "MediaPlay";
    big_play_->automation_id = "MediaPlay";
    big_play_->width = 110;
    big_play_->height = 38;
    big_play_->on_click = [this] { play(); };
}

MediaPane::~MediaPane() { stop(); }

void MediaPane::arrange(Rect r) {
    DockPanel::arrange(r);
    // The play button floats in the middle of the picture.
    const Rect s = screen_->bounds;
    big_play_->arrange(Rect::xywh(s.l + (s.width() - big_play_->width) / 2, s.t + (s.height() - big_play_->height) / 2, big_play_->width, big_play_->height));
}

bool MediaPane::setting(const char* key) const {
    BbOwnedStr o{};
    if (bb_settings_get(core_.app(), core::s(core::wide(key)), &o) != BB_OK) return false;
    return core::take(o).find(L"true") != std::wstring::npos;
}

void MediaPane::load_settings() {
    autoplay_on_ = setting("previewAutoPlay");
    loop_on_ = setting("previewLoop");
    advance_on_ = setting("previewAutoAdvance");
}

void MediaPane::toggle_setting(const char* key, const char* other) {
    const bool now = setting(key);
    // Loop and go-on-to-the-next are two answers to one question, so at most one is lit.
    if (!now && other) bb_settings_set(core_.app(), core::s(core::wide(other)), core::s(L"false"));
    bb_settings_set(core_.app(), core::s(core::wide(key)), core::s(now ? L"false" : L"true"));
    load_settings();
    if (player_) player_->set_loop(loop_on_);
    sync_transport();
}

void MediaPane::open(const std::wstring& path, ui::ThumbPtr poster, bool is_video) {
    stop();
    path_ = path;
    is_video_ = is_video;
    screen_->set_image(poster);
    big_play_->set_visible(true);
    transport_->set_visible(false);
    ended_handled_ = false;
    load_settings();
    sync_transport();
    request_layout();
    // Autoplay is for videos: arrowing down a folder of songs should not play each in turn.
    if (is_video && autoplay_on_) play();
}

void MediaPane::play() {
    if (path_.empty() || !root()) return;
    if (!player_) {
        player_ = std::make_unique<MediaPlayer>(res().dev);
        if (!player_->open(path_)) {
            player_.reset();
            return;
        }
        player_->set_volume(volume_->value());
        player_->set_loop(loop_on_);
    }
    started_ = true;
    ended_handled_ = false;
    player_->play();
    big_play_->set_visible(false);
    transport_->set_visible(true);
    start_ticking();
    sync_transport();
    request_layout();
}

void MediaPane::toggle_play() {
    if (!player_) {
        play();
        return;
    }
    if (player_->playing()) player_->pause();
    else {
        if (player_->ended()) player_->seek(0);
        player_->play();
        ended_handled_ = false;
    }
    sync_transport();
}

void MediaPane::stop() {
    if (full_) set_full_screen(false);
    if (root()) root()->stop_timer(this, 1);
    player_.reset();
    started_ = false;
    scrubbing_ = false;
    if (transport_) transport_->set_visible(false);
    if (big_play_) big_play_->set_visible(true);
}

void MediaPane::suspend() {
    if (player_ && player_->playing()) player_->pause();
    if (root()) root()->stop_timer(this, 1);
    sync_transport();
}

void MediaPane::start_ticking() {
    if (root()) root()->start_timer(this, 1, 33);
}

void MediaPane::on_timer(int id) {
    if (id == 1) tick();
}

std::wstring MediaPane::clock_text(double seconds) {
    const int total = static_cast<int>(std::max(0.0, seconds));
    wchar_t buf[32];
    if (total >= 3600) swprintf(buf, 32, L"%d:%02d:%02d", total / 3600, total / 60 % 60, total % 60);
    else swprintf(buf, 32, L"%d:%02d", total / 60, total % 60);
    return buf;
}

void MediaPane::tick() {
    if (!player_) return;
    if (player_->failed()) {
        time_->set_text(L"This file could not be played.");
        root()->stop_timer(this, 1);
        return;
    }
    if (ui::ThumbPtr frame = player_->take_frame()) {
        const ui::ThumbPtr& current = screen_->image();
        if (!current || current->width != frame->width || current->height != frame->height) screen_->set_image(frame);
        else screen_->set_frame(frame);
    }
    if (player_->ready()) {
        const double d = player_->duration();
        if (!scrubbing_ && d > 0) seek_->set_value(player_->position() / d * 1000.0);
        time_->set_text(clock_text(player_->position()) + L" / " + clock_text(d));
    }
    if (player_->ended() && !ended_handled_) {
        ended_handled_ = true;
        if (advance_on_ && on_next) on_next();
    }
    sync_transport();
}

void MediaPane::sync_transport() {
    const bool playing = player_ && player_->playing();
    play_->icon = icons::find(playing ? "Pause" : "Play");
    autoplay_->primary = autoplay_on_;
    loop_->primary = loop_on_;
    advance_->primary = advance_on_;
    fullscreen_->icon = icons::find(full_ ? "ExitFullScreen" : "FullScreen");
    invalidate();
}

// Pressed anywhere and dragged, the seek bar seeks as it goes rather than on release; a frame is a request to
// the engine, so it is thinned to one every few tens of milliseconds.
void MediaPane::seek_changed(double value, bool dragging) {
    if (!player_ || !player_->ready()) return;
    if (dragging && !scrubbing_) {
        resume_after_scrub_ = player_->playing();
        player_->pause();
        scrubbing_ = true;
    }
    const uint64_t now = root() ? root()->now_ms() : 0;
    if (!dragging || now - last_seek_ms_ >= 60) {
        player_->seek(value / 1000.0 * player_->duration());
        last_seek_ms_ = now;
    }
    if (!dragging && scrubbing_) {
        scrubbing_ = false;
        if (resume_after_scrub_) player_->play();
    }
}

void MediaPane::set_full_screen(bool on) {
    if (on == full_ || !root() || !parent()) return;
    if (on) {
        home_ = parent();
        std::unique_ptr<Widget> me = home_->remove_child(this);
        root()->content()->add_child(std::move(me));
        full_ = true;
        style.bg = tok::WindowBackground;
        root()->set_focus(this);
    } else {
        Widget* host = home_;
        std::unique_ptr<Widget> me = parent()->remove_child(this);
        full_ = false;
        style.bg = tok::SurfaceBackground;
        if (host) host->add_child(std::move(me));
    }
    sync_transport();
    request_layout();
    invalidate();
}

bool MediaPane::on_key_down(const KeyEvent& e) {
    if (e.vk == VK_ESCAPE && full_) {
        set_full_screen(false);
        return true;
    }
    if (e.vk == VK_SPACE && (full_ || started_)) {
        toggle_play();
        return true;
    }
    return false;
}

}  // namespace bb::views
