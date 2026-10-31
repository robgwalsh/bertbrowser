// Thumbnails for the tile view. The shell makes them (IShellItemImageFactory) on a small pool, four at a
// time because the shell is slow and apartment-bound; each finished image is posted to the UI thread and
// handed to the tile that asked. The harness swaps in a generator that draws a deterministic picture per row,
// so a capture never depends on what the machine's shell has cached.
#pragma once
#include "ui/collections/tile_view.h"

#include <windows.h>

#include <atomic>
#include <condition_variable>
#include <deque>
#include <functional>
#include <mutex>
#include <thread>
#include <vector>

namespace bb::views {

constexpr UINT WM_BB_THUMBS = WM_APP + 3;

enum class ThumbSource { Shell, Generated };

class ThumbnailService : public ui::ThumbnailProvider {
public:
    static constexpr size_t kSlots = 4;

    // `row_path` is asked on the UI thread, at request time, for the full path of a row index.
    ThumbnailService(HWND ui_window, ThumbSource source, std::function<std::wstring(size_t row)> row_path);
    ~ThumbnailService() override;

    void request(uint64_t row_id, size_t row, uint32_t px, ui::ThumbnailSink& sink) override;
    void cancel(uint64_t row_id) override;
    // On the UI thread, once WM_BB_THUMBS arrives: delivers what the workers finished.
    void drain();
    size_t pending() const;

    // The picture the generator draws for a name (exposed so a test can compare).
    static ui::ThumbPtr generate(const std::wstring& name, uint32_t px);

private:
    struct Job {
        uint64_t row_id;
        std::wstring path;
        uint32_t px;
        ui::ThumbnailSink* sink;
    };
    struct Done {
        uint64_t row_id;
        ui::ThumbnailSink* sink;
        ui::ThumbPtr image;
    };
    void work();
    ui::ThumbPtr shell_image(const std::wstring& path, uint32_t px);

    HWND window_;
    ThumbSource source_;
    std::function<std::wstring(size_t)> row_path_;
    mutable std::mutex mu_;
    std::condition_variable cv_;
    std::deque<Job> jobs_;
    std::deque<Done> done_;
    std::vector<uint64_t> cancelled_;
    std::vector<std::thread> threads_;
    std::atomic<bool> stop_{false};
    std::atomic<size_t> running_{0};
    bool notified_ = false;
};

}  // namespace bb::views
