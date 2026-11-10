// A tooltip with structure: a heading, a line under it, label/value rows in columns and a footnote. What a
// widget's make_rich_tip builds; the PopupLayer places and shows it like any tip, and rebuilds it on refresh_tip.
#pragma once
#include "ui/core/widget.h"

namespace bb::ui {

struct RichTipRow {
    std::wstring label;
    std::wstring value;
    // A second figure after the value (a total beside a rate). When there is one, the value sits in a cell as wide
    // as RichTipContent::value_template, so the detail does not move as the value's digits change.
    std::wstring detail;
};

struct RichTipContent {
    std::wstring title;
    std::wstring subtitle;  // optional: a status line under the title
    std::vector<RichTipRow> rows;
    std::wstring footer;          // optional: what clicking does
    std::wstring value_template;  // the widest value a row with a detail will show
};

class RichTip : public Widget {
public:
    RichTip(gfx::Resources& res, RichTipContent content);
    const RichTipContent& content() const { return content_; }
    Size natural_size() const { return natural_; }
    Size measure(Size) override { return natural_; }
    void paint(PaintContext& pc) override;

    static constexpr float kPadX = 10.0f, kPadY = 8.0f, kColumnGap = 16.0f, kDetailGap = 12.0f, kSectionGap = 6.0f;
    static constexpr float kMaxWidth = 420.0f;

private:
    gfx::Resources& res_;
    RichTipContent content_;
    gfx::FontSpec title_font_;
    float line_h_ = 0;
    float label_w_ = 0;
    float cell_w_ = 0;  // the value cell of a row with a detail
    Size natural_;
};

}  // namespace bb::ui
