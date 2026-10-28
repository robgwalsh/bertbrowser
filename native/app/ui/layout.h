// Layout containers. Stretching widgets (lists, trees, scroll viewers, edits in a row) report only
// their fixed size, or zero, from measure(): give them `flex` in a StackPanel or Dock::Fill in a DockPanel.
#pragma once
#include "ui/widget.h"

namespace bb::ui {

enum class Orientation { Horizontal, Vertical };

// A plain container. Children fill it (inset by `pad`); style.bg paints behind them.
class Panel : public Widget {
public:
    float pad = 0;
    Size measure(Size avail) override;
    void arrange(Rect r) override;
};

// Children in a row or column. A child takes its desired size along the axis unless it has `flex`, in
// which case the leftover is shared by flex weight; across the axis it stretches unless it has a fixed size.
class StackPanel : public Widget {
public:
    explicit StackPanel(Orientation o = Orientation::Vertical, float gap = 0) : orientation(o), spacing(gap) {}
    Orientation orientation;
    float spacing;
    float pad = 0;
    Size measure(Size avail) override;
    void arrange(Rect r) override;
};

// Children claim an edge in order (Widget::dock); the last Dock::Fill child gets what is left.
class DockPanel : public Widget {
public:
    float pad = 0;
    Size measure(Size avail) override;
    void arrange(Rect r) override;
};

}  // namespace bb::ui
