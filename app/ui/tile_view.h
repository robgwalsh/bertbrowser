// The thumbnail ("tiles") mode of the file list. It shares the ListView's RowProvider - same rows, same
// bulk selection calls - and lays the rows out as uniform tiles by arithmetic: with every tile the same
// size (the caption has a fixed height) the first and last visible tile are a division away, so a
// 100,000-row folder costs what the screen costs and nothing is ever measured. An image slot per tile is
// filled by a ThumbnailProvider asynchronously; until it arrives the slot shows a placeholder.
//
// What it holds: at most `max_in_flight` outstanding requests (the shell is slow and apartment-bound), only
// for realized tiles, and only the images of realized tiles - scrolling away drops them and cancels what
// has not come back, so memory is bounded by the screen however far the list was scrolled.
#pragma once
#include "ui/list_view.h"

#include <memory>
#include <unordered_map>

namespace bb::ui {

// A decoded thumbnail: BGRA, premultiplied alpha, tightly packed.
struct ThumbImage {
    uint32_t width = 0, height = 0;
    std::vector<uint8_t> bgra;
};
using ThumbPtr = std::shared_ptr<const ThumbImage>;

class ThumbnailSink {
public:
    virtual ~ThumbnailSink() = default;
    // UI thread only: the provider's worker posts to the UI thread and calls this there. A null image means
    // "no thumbnail exists" (the tile keeps its placeholder and is not asked again until it scrolls away).
    virtual void deliver(uint64_t row_id, ThumbPtr image) = 0;
};

class ThumbnailProvider {
public:
    virtual ~ThumbnailProvider() = default;
    // Start fetching a `px` x `px` thumbnail for the row. Returns at once; the answer goes to `sink.deliver`.
    virtual void request(uint64_t row_id, size_t row, uint32_t px, ThumbnailSink& sink) = 0;
    // The tile scrolled away before the answer came back; stop if it is cheap to.
    virtual void cancel(uint64_t /*row_id*/) {}
};

struct TileMetrics {
    float image = 104.0f;      // the square image slot
    float pad = 8.0f;          // around the image, and between image and caption
    float caption_h = 36.0f;   // fixed: two lines. Uniform tiles are what make the arithmetic work
    float gap = 6.0f;          // between tiles
    float margin = 8.0f;       // around the whole grid
    float tile_w() const { return image + 2 * pad; }
    float tile_h() const { return pad + image + 4 + caption_h + pad; }
};

// The pure arithmetic. Content coordinates: (0,0) is the grid's top-left before scrolling.
struct TileLayout {
    TileMetrics m;
    float width = 0;  // viewport width available to the grid
    size_t count = 0;

    int columns() const;
    size_t rows() const;
    float content_height() const;
    Rect tile_rect(size_t index) const;
    // Indices [first, last) of the tiles intersecting [scroll, scroll + viewport_h).
    std::pair<size_t, size_t> visible(float scroll, float viewport_h) const;
    // npos in a gap or past the end.
    size_t index_at(Point content_pt) const;
    Rect image_rect(size_t index) const;    // the image slot of a tile
    Rect caption_rect(size_t index) const;
};

class TileView : public Widget, public ThumbnailSink {
public:
    static constexpr size_t npos = static_cast<size_t>(-1);

    TileView();

    TileMetrics metrics;
    int caption_column = 0;        // the RowProvider column whose text captions a tile
    size_t max_in_flight = 4;
    int retain_extra_rows = 0;     // rows of tiles kept beyond the realized ones
    float dpi_scale = 1.0f;        // device pixels per DIP, for the size asked of the provider

    std::function<void(size_t row)> on_activate;

    void set_provider(RowProvider* p);
    void set_thumbnails(ThumbnailProvider* t);
    void refresh();  // the provider's rows changed

    size_t cursor() const { return cursor_; }
    void select_row(size_t row);   // cursor + selection + scroll into view
    void scroll_to_row(size_t row);
    void set_scroll_y(float y);
    float scroll_y() const { return v_.offset(); }

    // ---- geometry (window DIPs) ----
    TileLayout layout() const;
    Rect tile_rect(size_t row) const;  // where it is drawn now, scrolling applied
    size_t row_at(Point p) const;
    std::pair<size_t, size_t> realized() const;  // [first, last) rows with a container
    size_t painted_tiles() const { return painted_; }

    // ---- thumbnail bookkeeping (tests watch these) ----
    size_t in_flight() const { return in_flight_; }
    size_t cached() const { return cache_.size(); }  // entries held: requested or ready
    size_t ready_count() const;
    bool has_image(uint64_t row_id) const;

    // ThumbnailSink
    void deliver(uint64_t row_id, ThumbPtr image) override;

    Size measure(Size avail) override;
    void arrange(Rect r) override;
    void paint(PaintContext& pc) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_mouse_move(const MouseEvent& e) override;
    bool on_mouse_up(const MouseEvent& e) override;
    bool on_wheel(Point p, int delta, Mods m) override;
    bool on_key_down(const KeyEvent& e) override;
    void on_focus(bool) override { invalidate(); }

private:
    struct Entry {
        enum class State { Requested, Ready, None } state = State::Requested;
        ThumbPtr image;
        ComPtr<ID2D1Bitmap> bitmap;
        uint32_t generation = 0;
        size_t row = 0;
    };
    void layout_bar();
    void update_requests();   // trim to the realized rows, then ask for what is missing, up to the gate
    void move_cursor(size_t row, const Mods& m);
    size_t count() const { return provider_ ? provider_->row_count() : 0; }

    RowProvider* provider_ = nullptr;
    ThumbnailProvider* thumbs_ = nullptr;
    ScrollBar v_;
    Rect body_;
    size_t cursor_ = npos, anchor_ = npos;
    size_t painted_ = 0;
    bool bar_drag_ = false;
    bool updating_ = false;
    std::unordered_map<uint64_t, Entry> cache_;
    size_t in_flight_ = 0;
};

}  // namespace bb::ui
