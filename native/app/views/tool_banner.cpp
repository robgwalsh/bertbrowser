// The strip over the panes while two folders are being compared: what the comparison found, and the
// four things to do about it. It takes no room at all until there is something to put in it.
#include "views/forms.h"
#include "views/main_view.h"
#include "views/tool_dialogs.h"
#include "views/tool_state.h"

namespace bb::views {

using namespace bb::ui;
namespace f = forms;

namespace {

class CompareBanner : public StackPanel {
public:
    explicit CompareBanner(MainView& main) : StackPanel(Orientation::Horizontal, 10.0f), main_(main) {
        name = "CompareBanner";
        dock = Dock::Top;
        height = 40;
        pad = 6;
        style.bg = tok::SurfaceRaised;
        set_visible(false);
        text_ = f::text(*this, L"", tok::TextPrimary, 13.5f);
        text_->name = "CompareSummary";
        text_->flex = 1;
        text_->vcenter = true;
        only_ = f::check(*this, L"Differences only", false, [this](bool on) {
            if (updating_) return;
            try_tool_call(main_, "compare.filter", on ? "{\"on\":true}" : "{\"on\":false}");
        });
        only_->name = "CompareDifferencesOnly";
        rescan_ = f::button(*this, L"Rescan", [this] { try_tool_call(main_, "compare.rescan"); }, 84);
        rescan_->name = "CompareRescan";
        sync_ = f::button(*this, L"Sync…", [this] { main_.open_dialog(L"sync-preview"); }, 84);
        sync_->name = "CompareSync";
        end_ = f::button(*this, L"End", [this] { try_tool_call(main_, "compare.end"); }, 70);
        end_->name = "CompareEnd";
    }

    void sync() {
        const json::Value m = try_tool_call(main_, "compare.model");
        const bool active = m["active"].boolean();
        const bool changed = flags.visible != active;
        set_visible(active);
        if (active) {
            std::wstring words = m["comparing"].boolean() && !m["hasResult"].boolean() ? L"Comparing…" : core::wide(m["summary"].str());
            if (!m["message"].is_null()) words += L"  —  " + core::wide(m["message"].str());
            if (m["stale"].boolean()) words += L"  —  something changed since this was compared";
            text_->set_text(words);
            updating_ = true;
            only_->set_checked(m["differencesOnly"].boolean());
            updating_ = false;
            sync_->set_enabled(m["canSync"].boolean());
            rescan_->set_enabled(!m["comparing"].boolean());
        }
        if (changed && parent()) parent()->request_layout();
        invalidate();
    }

private:
    MainView& main_;
    TextBlock* text_ = nullptr;
    CheckBox* only_ = nullptr;
    Button *rescan_ = nullptr, *sync_ = nullptr, *end_ = nullptr;
    bool updating_ = false;
};

}  // namespace

std::unique_ptr<Widget> make_compare_banner(MainView& main) { return std::make_unique<CompareBanner>(main); }

void compare_banner_sync(Widget* banner) {
    if (auto* b = dynamic_cast<CompareBanner*>(banner)) b->sync();
}

}  // namespace bb::views
