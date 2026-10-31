// What the operations dialogs share: a card (title, body, buttons), a body that rebuilds itself when the
// core says its model moved, and the guard that answers a question if its dialog goes away unanswered.
#pragma once
#include "views/dialogs/dialogs.h"
#include "views/main/main_view.h"
#include "views/bridge/ops.h"

#include <functional>
#include <memory>
#include <string>
#include <vector>

namespace bb::views::ops {

// A card's root. When it is destroyed (the dialog closed, however it was closed) `on_destroy` runs, so a
// transfer thread parked on a question is never left waiting for an answer nobody can give.
class GuardedPanel : public ui::Panel {
public:
    std::function<void()> on_destroy;
    ~GuardedPanel() override {
        if (on_destroy) on_destroy();
    }
};

struct Card {
    std::unique_ptr<GuardedPanel> root;
    ui::StackPanel* body = nullptr;
    ui::StackPanel* buttons = nullptr;
};

Card make_card(const std::wstring& title, float width, std::unique_ptr<ui::StackPanel> body = nullptr);
ui::Button* add_button(Card& c, const wchar_t* label, std::function<void()> fn, bool primary = false, float width = 104.0f);

// A vertical body that refills itself from the core whenever `slot` of the main view fires. The link back
// is a token, not a pointer, so a card that outlives the view it was made for refreshes nothing.
class LiveBody : public ui::StackPanel {
public:
    using Slot = std::function<void()> MainView::*;
    LiveBody(MainView& main, Slot slot, std::function<void(LiveBody&)> fill);
    ~LiveBody() override;
    void refresh();
    std::function<void()> after;  // runs after every refill: the footer buttons catch up with the model

private:
    struct Link {
        LiveBody* body;
    };
    std::shared_ptr<Link> link_;
    std::function<void(LiveBody&)> fill_;
};

// The dialogs, one function each (ops_dialogs.cpp, ops_forms.cpp).
std::unique_ptr<ui::Widget> transfer_dialog(DialogContext& cx);
std::unique_ptr<ui::Widget> conflicts_dialog(DialogContext& cx);
std::unique_ptr<ui::Widget> elevation_dialog(DialogContext& cx);
std::unique_ptr<ui::Widget> history_dialog(DialogContext& cx);
std::unique_ptr<ui::Widget> rename_dialog(DialogContext& cx, bool expanded);
std::unique_ptr<ui::Widget> compress_dialog(DialogContext& cx);
std::unique_ptr<ui::Widget> extract_dialog(DialogContext& cx);
std::unique_ptr<ui::Widget> password_dialog(DialogContext& cx);

// What an extract is about: the archive the tab is inside (with the selection in it, or everything), or the one
// archive selected in an ordinary folder. No archive: nothing to extract.
struct ExtractSubject {
    std::wstring archive;
    std::vector<std::wstring> names;
    bool inside = false;
};
ExtractSubject extract_subject(MainView& main);
// Where Extract here puts it (beside, or a new folder named after it). Spec: ExtractRules.DestinationFor.
std::wstring extract_destination(MainView& main, const std::wstring& archive);
void run_extract(MainView& main, const ExtractSubject& s, const std::wstring& destination, bool skip);

}  // namespace bb::views::ops
