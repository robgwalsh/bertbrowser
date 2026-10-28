#include "fixture.h"

using namespace bb;
using namespace bb::ui;

namespace {

size_t count_pixels(Rig& rig, int r0, int g0, int b0, int tol) {
    std::vector<uint8_t> px;
    rig.bitmap().read_pixels(px);
    size_t n = 0;
    for (size_t i = 0; i + 3 < px.size(); i += 4) {
        if (std::abs(px[i + 2] - r0) <= tol && std::abs(px[i + 1] - g0) <= tol && std::abs(px[i] - b0) <= tol) ++n;
    }
    return n;
}

}  // namespace

TEST(edit_typing_clusters_selection_and_undo) {
    Rig rig(400, 60);
    auto* e = rig.set(std::make_unique<Edit>());
    rig.root.set_focus(e);

    // Typing arrives as WM_CHAR, a surrogate pair as two messages.
    rig.type(L"Hello wörld \U0001F600");
    CHECK_TEXT(e->text(), L"Hello wörld \U0001F600");
    CHECK_EQ(e->caret(), e->text().size());

    // Backspace over an emoji is one cluster, two code units.
    rig.type(L"\b");
    CHECK_TEXT(e->text(), L"Hello wörld ");

    rig.type(L"\U0001F600");
    rig.key(VK_LEFT);
    CHECK_EQ(e->caret(), e->text().size() - 2);  // left steps over the pair
    rig.key(VK_LEFT, false, true);
    CHECK_EQ(e->caret(), 6);  // ctrl+left: start of the word
    rig.key(VK_END);

    rig.key(VK_HOME, true);
    CHECK_TEXT(e->selected_text(), e->text());
    rig.type(L"X");
    CHECK_TEXT(e->text(), L"X");  // typing replaces the selection

    // Undo is grouped by kind: the replacement, the retyped emoji, the backspace, the typing run.
    rig.key('Z', false, true);
    CHECK_TEXT(e->text(), L"Hello wörld \U0001F600");
    CHECK_TEXT(e->selected_text(), e->text());  // undo restores the selection
    rig.key('Z', false, true);
    CHECK_TEXT(e->text(), L"Hello wörld ");
    rig.key('Z', false, true);
    CHECK_TEXT(e->text(), L"Hello wörld \U0001F600");
    rig.key('Z', false, true);
    CHECK_TEXT(e->text(), L"");
    rig.key('Z', false, true);
    CHECK_TEXT(e->text(), L"");  // past the beginning is a no-op
    rig.key('Y', false, true);
    CHECK_TEXT(e->text(), L"Hello wörld \U0001F600");
    rig.key('Y', false, true);
    CHECK_TEXT(e->text(), L"Hello wörld ");

    // Cut, copy, paste through the clipboard seam; line breaks flatten on one line.
    rig.key(VK_HOME);
    rig.key(VK_RIGHT, true, true);
    CHECK_TEXT(e->selected_text(), L"Hello ");
    rig.key('X', false, true);
    CHECK_TEXT(rig.clip, L"Hello ");
    CHECK_TEXT(e->text(), L"wörld ");
    rig.key(VK_END);
    rig.clip = L"a\r\nb\nc";
    rig.key('V', false, true);
    CHECK_TEXT(e->text(), L"wörld a b c");
    rig.key('Z', false, true);
    CHECK_TEXT(e->text(), L"wörld ");  // one undo step for the paste
    rig.key('Z', false, true);
    CHECK_TEXT(e->text(), L"Hello wörld ");
    rig.key('A', false, true);
    rig.key('C', false, true);
    CHECK_TEXT(rig.clip, L"Hello wörld ");

    // Collapsing a selection with the arrow keys.
    rig.key(VK_LEFT);
    CHECK_EQ(e->caret(), 0);
    CHECK_EQ(e->anchor(), 0);
}

TEST(edit_ime_composition) {
    Rig rig(400, 60);
    auto* e = rig.set(std::make_unique<Edit>());
    rig.root.set_focus(e);
    rig.type(L"Hello ");
    rig.root.ime_composition(L"にほ", 1);
    CHECK(e->composing());
    CHECK_TEXT(e->text(), L"Hello ");  // the composition is display-only until it commits
    Rect cr;
    CHECK(rig.root.ime_caret_rect(&cr));  // the window positions the candidate list from this
    const float mid = cr.l;
    rig.root.ime_composition(L"にほん", 3);
    Rect cr2;
    rig.root.ime_caret_rect(&cr2);
    CHECK(cr2.l > mid);  // the caret follows the composition cursor
    rig.type(L"zzz");
    CHECK_TEXT(e->text(), L"Hello ");  // the IME owns the keyboard meanwhile
    rig.root.ime_commit(L"日本");
    rig.root.ime_end();
    CHECK(!e->composing());
    CHECK_TEXT(e->text(), L"Hello 日本");
    rig.key('Z', false, true);
    CHECK_TEXT(e->text(), L"Hello ");  // a commit is one undo step
}

TEST(edit_scrolls_to_keep_the_caret_visible) {
    Rig rig(300, 60);
    auto* e = rig.set(std::make_unique<Edit>());
    rig.root.set_focus(e);
    rig.type(L"short");
    CHECK_NEAR(e->scroll_x(), 0.0, 0.001);
    rig.type(L" and then a great deal more text than fits in the box at all");
    CHECK(e->scroll_x() > 0);
    const Rect c = e->caret_rect();
    CHECK(c.l >= e->bounds.l && c.l <= e->bounds.r);
    rig.key(VK_HOME);
    CHECK_NEAR(e->scroll_x(), 0.0, 0.001);
}

TEST(edit_mouse_caret_drag_and_word) {
    Rig rig(400, 60);
    auto* e = rig.set(std::make_unique<Edit>());
    e->set_text(L"alpha beta gamma");
    auto x_of = [&](size_t pos) {
        e->select(pos, pos);
        return e->caret_rect();
    };
    const Rect r6 = x_of(6);
    const float y = (r6.t + r6.b) / 2;
    rig.click({r6.l + 1, y});
    CHECK_EQ(e->caret(), 6);
    CHECK(rig.root.focused() == e);
    const Rect r0 = x_of(0), r5 = x_of(5);
    rig.down({r0.l + 1, y});
    rig.move({r5.l + 1, y});
    rig.up({r5.l + 1, y});
    CHECK_TEXT(e->selected_text(), L"alpha");
    const Rect r7 = x_of(7);
    rig.down({r7.l + 1, y}, {}, 2);  // double click inside "beta"
    rig.up({r7.l + 1, y});
    CHECK_TEXT(e->selected_text(), L"beta");
}

TEST(edit_multiline) {
    Rig rig(300, 120);
    auto* e = rig.set(std::make_unique<Edit>(true));
    rig.root.set_focus(e);
    rig.type(L"line1");
    rig.key(VK_RETURN);
    rig.type(L"line2");
    CHECK_TEXT(e->text(), L"line1\nline2");
    rig.key(VK_UP);
    CHECK(e->caret() <= 5);  // moved up a line, same column
    rig.key(VK_DOWN);
    CHECK(e->caret() >= 6);
    rig.key(VK_HOME);
    CHECK_EQ(e->caret(), 6);  // start of the line, not the document
    rig.key(VK_HOME, false, true);
    CHECK_EQ(e->caret(), 0);
    rig.key(VK_END, false, true);
    rig.clip = L"a\r\nb";
    rig.key('V', false, true);
    CHECK_TEXT(e->text(), L"line1\nline2a\nb");  // breaks kept, normalised to \n
    // Many lines scroll vertically to keep the caret in view.
    for (int i = 0; i < 12; ++i) {
        rig.key(VK_RETURN);
        rig.type(L"more");
    }
    CHECK(e->scroll_y() > 0);
    rig.key(VK_HOME, false, true);
    CHECK_NEAR(e->scroll_y(), 0.0, 0.001);
    rig.render();
    CHECK(!rig.bitmap().is_flat());
}

TEST(edit_callbacks_and_read_only) {
    Rig rig(300, 60);
    auto* e = rig.set(std::make_unique<Edit>());
    rig.root.set_focus(e);
    int changed = 0, committed = 0, cancelled = 0;
    e->on_changed = [&](const std::wstring&) { ++changed; };
    e->on_commit = [&] { ++committed; };
    e->on_cancel = [&] { ++cancelled; };
    rig.type(L"ab");
    CHECK_EQ(changed, 2);
    rig.key(VK_RETURN);
    rig.key(VK_ESCAPE);
    CHECK_EQ(committed, 1);
    CHECK_EQ(cancelled, 1);
    e->read_only = true;
    rig.type(L"c");
    rig.key(VK_DELETE);
    CHECK_TEXT(e->text(), L"ab");
    rig.key('A', false, true);
    rig.key('C', false, true);
    CHECK_TEXT(rig.clip, L"ab");  // copying still works
}

TEST(edit_paints_selection_composition_and_placeholder) {
    Rig rig(420, 170);
    auto col = std::make_unique<StackPanel>(Orientation::Vertical, 14.0f);
    col->pad = 20;
    auto* sel = col->emplace<Edit>();
    auto* comp = col->emplace<Edit>();
    auto* empty = col->emplace<Edit>();
    sel->set_text(L"Hello wörld");
    sel->select(6, 11);
    comp->set_text(L"Type ");
    comp->set_composition(L"にほんご", 2);
    empty->placeholder = L"Search";
    rig.set(std::move(col));
    rig.root.set_focus(sel);
    rig.render();
    const uint32_t s = Env::get().res->palette.argb(tok::InputSelectionBackground);
    CHECK(count_pixels(rig, (s >> 16) & 255, (s >> 8) & 255, s & 255, 4) > 100);  // selection highlight
    const uint32_t c = Env::get().res->palette.argb(tok::BorderActive);
    CHECK(count_pixels(rig, (c >> 16) & 255, (c >> 8) & 255, c & 255, 6) > 20);   // composition, underlined
    const uint32_t p = Env::get().res->palette.argb(tok::TextPlaceholder);
    CHECK(count_pixels(rig, (p >> 16) & 255, (p >> 8) & 255, p & 255, 30) > 20);  // placeholder text
}
