#include "views/image_view.h"

#include "bbcore.h"

#include <windows.h>

#include <algorithm>
#include <cmath>

namespace bb::views {

using namespace ui;

ImageView::ImageView() {
    flags.focusable = true;
    flags.clip_children = true;
    style.bg = tok::SurfaceBackground;
}

void ImageView::set_image(ThumbPtr image) {
    image_ = std::move(image);
    bitmap_.Reset();
    dirty_ = true;
    actual_ = false;
    actual_pending_ = false;
    scale_ = 1.0;
    ox_ = oy_ = 0;
    invalidate();
}

void ImageView::set_frame(ThumbPtr frame) {
    image_ = std::move(frame);
    dirty_ = true;
    invalidate();
}

// Fit is a scale of 1 over the fitted layout; 100% zooms about the middle of the pane to one picture pixel per
// DIP. Spec: PreviewPane.ApplyImageFit.
void ImageView::set_fitted(bool fitted) {
    if (actual_ == !fitted) return;
    actual_ = !fitted;
    scale_ = 1.0;
    ox_ = oy_ = 0;
    actual_pending_ = false;
    if (actual_) apply_actual();
    invalidate();
    if (on_fit_changed) on_fit_changed();
}

double ImageView::one_to_one() const {
    const Rect f = fitted_rect();
    return image_ && f.width() > 0 ? static_cast<double>(image_->width) / f.width() : 1.0;
}

void ImageView::apply_actual() {
    const Rect f = fitted_rect();
    if (f.empty()) {
        actual_pending_ = true;
        return;
    }
    actual_pending_ = false;
    const double one = one_to_one();
    apply_scale(bb_zoom_clamp(one, one), {bounds.l + bounds.width() / 2, bounds.t + bounds.height() / 2}, one);
}

bool ImageView::can_pan() const {
    const Rect f = fitted_rect();
    return f.width() * scale_ > bounds.width() + 0.5 || f.height() * scale_ > bounds.height() + 0.5;
}

float ImageView::natural_width() const { return image_ ? static_cast<float>(image_->width) + 16.0f : 0.0f; }

Rect ImageView::fitted_rect() const {
    if (!image_ || !image_->width || !image_->height) return {};
    const float vw = bounds.width(), vh = bounds.height();
    const float iw = static_cast<float>(image_->width), ih = static_cast<float>(image_->height);
    float s = std::min(vw / iw, vh / ih);
    if (!upscale) s = std::min(s, 1.0f);
    const float w = iw * s, h = ih * s;
    return Rect::xywh(bounds.l + (vw - w) / 2, bounds.t + (vh - h) / 2, w, h);
}

void ImageView::clamp_offset() {
    const Rect f = fitted_rect();
    if (f.empty()) return;
    ox_ = bb_zoom_clamp_offset(ox_, f.l - bounds.l, f.width(), scale_, bounds.width());
    oy_ = bb_zoom_clamp_offset(oy_, f.t - bounds.t, f.height(), scale_, bounds.height());
}

void ImageView::arrange(Rect r) {
    bounds = r;
    if (actual_pending_) apply_actual();
    clamp_offset();
}

void ImageView::apply_scale(double scale, Point anchor, double min_scale) {
    const Rect f = fitted_rect();
    if (f.empty()) return;
    const double old_scale = scale_;
    scale = bb_zoom_clamp(scale, min_scale);
    if (scale == old_scale) return;
    // The image point under the cursor, in the picture's own (untransformed) coordinates.
    const double ax = (anchor.x - (f.l + ox_)) / old_scale;
    const double ay = (anchor.y - (f.t + oy_)) / old_scale;
    ox_ = bb_zoom_offset(ox_, ax, old_scale, scale);
    oy_ = bb_zoom_offset(oy_, ay, old_scale, scale);
    scale_ = scale;
    clamp_offset();
    invalidate();
}

void ImageView::paint(PaintContext& pc) {
    Widget::paint(pc);
    if (!image_ || !image_->width || !image_->height) return;

    const bool same_size = bitmap_ && bitmap_->GetPixelSize().width == image_->width && bitmap_->GetPixelSize().height == image_->height;
    if (!bitmap_ || generation_ != pc.res.dev.generation() || (dirty_ && !same_size)) {
        bitmap_.Reset();
        const D2D1_BITMAP_PROPERTIES props =
            D2D1::BitmapProperties(D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED));
        pc.dc->CreateBitmap(D2D1::SizeU(image_->width, image_->height), image_->bgra.data(), image_->width * 4, props, &bitmap_);
        generation_ = pc.res.dev.generation();
        dirty_ = false;
    } else if (dirty_) {
        bitmap_->CopyFromMemory(nullptr, image_->bgra.data(), image_->width * 4);
        dirty_ = false;
    }
    if (!bitmap_) return;

    const Rect f = fitted_rect();
    const Rect dest = Rect::xywh(f.l + static_cast<float>(ox_), f.t + static_cast<float>(oy_), f.width() * static_cast<float>(scale_),
                                 f.height() * static_cast<float>(scale_));
    pc.clip(bounds);

    // The chequerboard under the picture, only where the picture is.
    const Rect seen = dest.intersect(bounds);
    if (checkerboard && !seen.empty()) {
        constexpr float kSquare = 8.0f;
        pc.fill(seen, tok::PreviewCheckerLight);
        const int c0 = static_cast<int>(std::floor((seen.l - dest.l) / kSquare)), c1 = static_cast<int>(std::ceil((seen.r - dest.l) / kSquare));
        const int r0 = static_cast<int>(std::floor((seen.t - dest.t) / kSquare)), r1 = static_cast<int>(std::ceil((seen.b - dest.t) / kSquare));
        for (int r = r0; r < r1; ++r) {
            for (int c = c0; c < c1; ++c) {
                if (((r + c) & 1) == 0) continue;
                const Rect cell = Rect::xywh(dest.l + static_cast<float>(c) * kSquare, dest.t + static_cast<float>(r) * kSquare, kSquare, kSquare).intersect(seen);
                if (!cell.empty()) pc.fill(cell, tok::PreviewCheckerDark);
            }
        }
    }
    pc.dc->DrawBitmap(bitmap_.Get(), dest.d2d(), 1.0f, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR);
    pc.unclip();
}

bool ImageView::on_mouse_down(const MouseEvent& e) {
    if (!interactive || e.button != MouseButton::Left) return false;
    root()->set_focus(this);
    if (e.clicks >= 2) {
        if (on_fit_request) on_fit_request();
        else toggle_fit();
        return true;
    }
    if (can_pan()) {
        panning_ = true;
        pan_from_ = e.pt;
        pan_ox_ = ox_;
        pan_oy_ = oy_;
        root()->capture(this);
    }
    return true;
}

bool ImageView::on_mouse_move(const MouseEvent& e) {
    if (!panning_) return false;
    ox_ = pan_ox_ + (e.pt.x - pan_from_.x);
    oy_ = pan_oy_ + (e.pt.y - pan_from_.y);
    clamp_offset();
    invalidate();
    return true;
}

bool ImageView::on_mouse_up(const MouseEvent&) {
    if (!panning_) return false;
    panning_ = false;
    root()->release_capture();
    return true;
}

// Zooms about the point under the cursor. Fit is the floor unless 100% is smaller than fit, and zooming a
// fitted picture leaves "fit", so the button and a double-click both mean "back to fit". Spec:
// PreviewPane.Image_MouseWheel.
bool ImageView::on_wheel(Point p, int delta, Mods) {
    if (!interactive || !image_) return false;
    const double floor = std::min(1.0, one_to_one());
    const double before = scale_;
    apply_scale(bb_zoom_step(scale_, static_cast<double>(delta) / 120.0, floor), p, floor);
    if (scale_ != before && !actual_) {
        actual_ = true;
        if (on_left_fit) on_left_fit();
        if (on_fit_changed) on_fit_changed();
    }
    return true;
}

bool ImageView::on_key_down(const KeyEvent& e) {
    if (e.vk == VK_SPACE && on_toggle_playing) {
        on_toggle_playing();
        return true;
    }
    return false;
}

Cursor ImageView::cursor_at(Point) const { return panning_ ? Cursor::SizeAll : can_pan() ? Cursor::Hand : Cursor::Arrow; }

}  // namespace bb::views
