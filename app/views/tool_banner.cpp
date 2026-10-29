// The strip over the panes while two folders are being compared: what the comparison found, and the
// four things to do about it, with any caveat in a strip of its own below. It takes no room at all
// until there is something to put in it. Spec: MainWindow.xaml (CompareBanner).
#include "views/forms.h"
#include "gfx/icons.gen.h"
#include "views/main_view.h"
#include "views/tool_dialogs.h"
#include "views/tool_state.h"

namespace bb::views {

using namespace bb::ui;
namespace f = forms;

namespace {

class CompareBanner : public StackPanel {
public:
    explicit CompareBanner(MainView& main) : StackPanel(Orientation::Vertical, 0.0f), main_(main) {
        name = "CompareBanner";
        dock = Dock::Top;
        set_visible(false);
        auto* row = emplace<StackPanel>(Orientation::Horizontal, 12.0f);
        row->height = 36;
        row->pad = 6;
        row->style.bg = tok::SurfaceRaised;
        auto* icon = row->emplace<Icon>(&icons::Compare, 16.0f);
        icon->style.fg = tok::TextPrimary;
        text_ = f::text(*row, L"", tok::TextPrimary, 13.5f);
        text_->name = "CompareSummary";
        text_->flex = 1;
        text_->vcenter = true;
        only_ = f::check(*row, L"Only differences", false, [this](bool on) {
            if (updating_) return;
            try_tool_call(main_, "compare.filter", on ? "{\"on\":true}" : "{\"on\":false}");
        });
        only_->name = "CompareDifferencesOnly";
        sync_ = f::button(*row, L"Sync →", [this] { main_.open_dialog(L"sync-preview"); }, 90);
        sync_->name = "CompareSync";
        sync_->tooltip = L"Make the right side match the left";
        rescan_ = f::button(*row, L"Rescan", [this] { try_tool_call(main_, "compare.rescan"); });
        rescan_->name = "CompareRescan";
        rescan_->link = true;
        rescan_->tooltip = L"Read both folders again";
        end_ = f::button(*row, L"End", [this] { try_tool_call(main_, "compare.end"); });
        end_->name = "CompareEnd";
        end_->link = true;
        end_->tooltip = L"Stop comparing and clear the colours";
        // Said separately from the summary: a caveat changes what the numbers beside it are worth, and
        // running the two together reads as one sentence.
        caveat_panel_ = emplace<StackPanel>(Orientation::Horizontal, 0.0f);
        caveat_panel_->pad = 4;
        caveat_panel_->style.bg = tok::WarningBackground;
        caveat_ = f::text(*caveat_panel_, L"", tok::WarningForeground, 13.0f);
        caveat_->name = "CompareCaveat";
        caveat_->wrap = true;
        caveat_->flex = 1;
        auto* rule = emplace<Widget>();
        rule->height = 1;
        rule->style.bg = tok::BorderSubtle;
    }

    void sync() {
        const json::Value m = try_tool_call(main_, "compare.model");
        const bool active = m["active"].boolean();
        bool changed = flags.visible != active;
        set_visible(active);
        if (active) {
            text_->set_text(m["comparing"].boolean() && !m["hasResult"].boolean() ? L"Comparing…" : core::wide(m["summary"].str()));
            std::wstring caveat = m["message"].is_null() ? std::wstring() : core::wide(m["message"].str());
            if (m["stale"].boolean()) {
                if (!caveat.empty()) caveat += L"  ";
                caveat += L"Something changed since this was compared; Rescan to read both folders again.";
            }
            if (caveat_->text() != caveat) caveat_->set_text(caveat);
            changed |= caveat_panel_->flags.visible != !caveat.empty();
            caveat_panel_->set_visible(!caveat.empty());
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
    TextBlock* caveat_ = nullptr;
    StackPanel* caveat_panel_ = nullptr;
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
