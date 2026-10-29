// A picture in the preview pane: laid out fitted to the pane, with zoom and pan as a transform over that
// layout (the rules are the core's `ImageZoom`), and a chequerboard behind it so transparency shows as
// transparency. An animated GIF plays in this same widget — its frames are swapped in place — so zoom, pan
// and fit apply to it exactly as to a still.
#pragma once
#include "ui/tile_view.h"
#include "ui/widget.h"

#include <functional>

namespace bb::views {

class ImageView : public ui::Widget {
public:
    ImageView();

    // A new picture: back to fitted, centred.
    void set_image(ui::ThumbPtr image);
    // The next frame of the same picture: zoom and pan stay where they are.
    void set_frame(ui::ThumbPtr frame);
    // The picture's own size, for the pane's subtitle and a GIF whose frames are the canvas.
    const ui::ThumbPtr& image() const { return image_; }

    // Fitted to the pane (the default) or shown at 100%.
    bool fitted() const { return !actual_; }
    void set_fitted(bool fitted);
    void toggle_fit() { set_fitted(actual_); }
    // The scale relative to "fit": 1 is fitted, and it never goes below that.
    double scale() const { return scale_; }
    // The width the image would want at 100%, plus the pane's own margins: for "fit width".
    float natural_width() const;

    // False for a thumbnail-sized cover that only sits there: no wheel zoom, no pan, no double-click.
    bool interactive = true;
    bool checkerboard = true;  // the chequerboard under a picture with transparency (off for an icon)

    std::function<void()> on_toggle_playing;  // Space, when the picture is animated
    std::function<void()> on_fit_changed;
    std::function<void()> on_fit_request;  // a double click: ask whoever owns the switch (else toggled here)

    ui::Size measure(ui::Size) override { return fixed_or({0, 0}); }
    void arrange(ui::Rect r) override;
    void paint(ui::PaintContext& pc) override;
    bool on_mouse_down(const ui::MouseEvent& e) override;
    bool on_mouse_move(const ui::MouseEvent& e) override;
    bool on_mouse_up(const ui::MouseEvent& e) override;
    bool on_wheel(ui::Point p, int delta, ui::Mods m) override;
    bool on_key_down(const ui::KeyEvent& e) override;
    ui::Cursor cursor_at(ui::Point p) const override;

private:
    ui::Rect fitted_rect() const;  // the picture laid out to fit, before zoom and pan
    void clamp_offset();
    void apply_scale(double scale, ui::Point anchor);

    ui::ThumbPtr image_;
    Microsoft::WRL::ComPtr<ID2D1Bitmap> bitmap_;
    uint32_t generation_ = 0;
    bool dirty_ = true;
    bool actual_ = false;
    double scale_ = 1.0;
    double ox_ = 0, oy_ = 0;  // translation in DIPs, applied after the scale about the fitted rect's origin
    bool panning_ = false;
    ui::Point pan_from_;
    double pan_ox_ = 0, pan_oy_ = 0;
};

}  // namespace bb::views
