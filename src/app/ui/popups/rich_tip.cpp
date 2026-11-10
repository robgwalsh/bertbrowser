#include "ui/popups/rich_tip.h"

#include <algorithm>
#include <cmath>

namespace bb::ui {

RichTip::RichTip(gfx::Resources& res, RichTipContent content) : res_(res), content_(std::move(content)) {
    flags.hit_testable = false;
    title_font_ = style.font;
    title_font_.weight = 600;
    const auto measure = [&](const std::wstring& s, const gfx::FontSpec& f) { return res_.text.measure(s, f); };

    line_h_ = std::ceil(measure(L"Ag", style.font).height);
    float widest = 0;
    float h = 0;
    if (!content_.title.empty()) {
        widest = std::max(widest, measure(content_.title, title_font_).width);
        h += line_h_;
    }
    if (!content_.subtitle.empty()) {
        widest = std::max(widest, measure(content_.subtitle, style.font).width);
        h += line_h_;
    }
    cell_w_ = content_.value_template.empty() ? 0.0f : std::ceil(measure(content_.value_template, style.font).width);
    for (const auto& r : content_.rows) {
        if (!r.detail.empty()) cell_w_ = std::max(cell_w_, std::ceil(measure(r.value, style.font).width));
    }
    float values_w = 0;
    for (const auto& r : content_.rows) {
        label_w_ = std::max(label_w_, measure(r.label, style.font).width);
        const float v = r.detail.empty() ? measure(r.value, style.font).width
                                         : cell_w_ + kDetailGap + measure(r.detail, style.font).width;
        values_w = std::max(values_w, v);
    }
    label_w_ = std::ceil(label_w_);
    if (!content_.rows.empty()) {
        widest = std::max(widest, label_w_ + kColumnGap + values_w);
        if (h > 0) h += kSectionGap;
        h += line_h_ * static_cast<float>(content_.rows.size());
    }
    if (!content_.footer.empty()) {
        widest = std::max(widest, measure(content_.footer, style.font).width);
        if (h > 0) h += kSectionGap;
        h += line_h_;
    }
    natural_ = {std::min(std::ceil(widest) + 2 * kPadX, kMaxWidth), std::ceil(h) + 2 * kPadY};
}

void RichTip::paint(PaintContext& pc) {
    pc.fill(bounds, tok::OverlayBackground);
    pc.stroke(bounds, tok::OverlayBorder);
    const float l = bounds.l + kPadX, r = bounds.r - kPadX;
    float y = bounds.t + kPadY;
    const auto line = [&](const std::wstring& s, const gfx::FontSpec& f, Token fg) {
        pc.text(s, Rect{l, y, r, y + line_h_}, f, fg);
        y += line_h_;
    };
    if (!content_.title.empty()) line(content_.title, title_font_, tok::TextPrimary);
    if (!content_.subtitle.empty()) line(content_.subtitle, style.font, tok::TextSecondary);
    if (!content_.rows.empty()) {
        if (y > bounds.t + kPadY) y += kSectionGap;
        const float values = std::min(l + label_w_ + kColumnGap, r);
        for (const auto& row : content_.rows) {
            pc.text(row.label, Rect{l, y, values - kColumnGap, y + line_h_}, style.font, tok::TextSecondary);
            if (row.detail.empty()) {
                pc.text(row.value, Rect{values, y, r, y + line_h_}, style.font, tok::TextPrimary);
            } else {
                const float detail = std::min(values + cell_w_ + kDetailGap, r);
                pc.text(row.value, Rect{values, y, detail - kDetailGap, y + line_h_}, style.font, tok::TextPrimary);
                pc.text(row.detail, Rect{detail, y, r, y + line_h_}, style.font, tok::TextSecondary);
            }
            y += line_h_;
        }
    }
    if (!content_.footer.empty()) {
        y += kSectionGap;
        line(content_.footer, style.font, tok::TextMuted);
    }
}

}  // namespace bb::ui
