// Video and sound for the preview pane without a window of the engine's own: Media Foundation's media
// engine in frame-server mode, the current frame copied into a Direct3D 11 texture and read back as pixels
// the pane draws like any other picture (spike S2 proved this on hardware and on WARP; never ask for video
// support on WARP, which refuses it).
//
// Nothing is opened until `open`, which the pane calls when someone presses play, so selecting a video holds
// no file, starts no pipeline and makes no sound.
#pragma once
#include "gfx/device.h"
#include "ui/collections/tile_view.h"

#include <mfmediaengine.h>

#include <atomic>
#include <memory>
#include <string>

namespace bb::views {

class MediaPlayer {
public:
    explicit MediaPlayer(gfx::GfxDevice& dev);
    ~MediaPlayer();
    MediaPlayer(const MediaPlayer&) = delete;
    MediaPlayer& operator=(const MediaPlayer&) = delete;

    // Starts loading `path`; false if the engine could not be made. Loading finishes on its own thread:
    // `ready()` says when the length and the size are known.
    bool open(const std::wstring& path);
    void close();

    bool ready() const { return ready_ && !failed_; }
    bool failed() const { return failed_; }
    bool ended() const;
    bool has_video() const { return video_w_ > 0; }
    uint32_t video_width() const { return video_w_; }
    uint32_t video_height() const { return video_h_; }

    void play();
    void pause();
    bool playing() const;
    double position() const;
    double duration() const;
    void seek(double seconds);
    void set_volume(double v);
    double volume() const;
    void set_loop(bool loop);

    // The newest frame if one has arrived since the last call (premultiplied BGRA), else null.
    ui::ThumbPtr take_frame();

private:
    class Notify;
    struct Impl;
    std::unique_ptr<Impl> impl_;
    std::atomic<bool> ready_{false}, failed_{false};
    uint32_t video_w_ = 0, video_h_ = 0;
    gfx::GfxDevice& dev_;
};

}  // namespace bb::views
