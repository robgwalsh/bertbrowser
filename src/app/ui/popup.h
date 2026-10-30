// What a popup (a menu level, a drop-down list, a tooltip) is shown through. Two presenters implement it:
//   PopupLayer (popup_layer.h)   draws the popup as a child of the owner's own tree - no window at all, so a
//                                menu can be photographed on a BitmapSurface like every other widget;
//   WindowPresenter (platform/popup_window.h)   gives each popup its own owned, no-activate Win32 window
//                                drawn by D2D, so it can leave the owner's frame. TrackPopupMenuEx is never used.
// All rectangles are in the owner's client DIPs (the same space as every Widget::bounds), even when the
// popup is a window that sticks out of the owner.
#pragma once
#include "ui/widget.h"

namespace bb::ui {

class PopupPresenter {
public:
    virtual ~PopupPresenter() = default;
    // The area popups should stay inside (the monitor's work area, or the layer's bounds), owner DIPs.
    virtual Rect work_area() = 0;
    // Takes ownership of `content` and shows it at `rect`. The returned pointer is `content`.
    virtual Widget* show(std::unique_ptr<Widget> content, Rect rect) = 0;
    // Hides and destroys a popup shown earlier.
    virtual void hide(Widget* content) = 0;
    // One timer for the controller's submenu hover delay; on firing the owner calls MenuController::timer().
    virtual void start_timer(unsigned ms) = 0;
    virtual void stop_timer() = 0;
};

enum class PopupSide {
    Below,   // under the anchor, left edges aligned (a menu bar heading, a combo box)
    Beside,  // to the right of the anchor, top aligned (a submenu); flips left when it would not fit
};

// Where a popup of `size` goes for `anchor` inside `work`. Pure; flips and clamps, never leaves `work`
// unless it is bigger than it (then it is pinned to the top-left).
Rect place_popup(Size size, Rect anchor, PopupSide side, Rect work);

}  // namespace bb::ui
