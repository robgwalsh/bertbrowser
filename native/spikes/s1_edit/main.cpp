// Spike S1: a single-line edit control over IDWriteTextLayout. The app has ~45 of these (search box,
// path box, rename, settings fields, the palette) and WPF gave them for free, so the risk is that a
// custom one is subtly wrong in the ways users notice: surrogate pairs split, caret lost off the right
// edge, IME composition drawn somewhere else, undo grouping that undoes one letter at a time.
//
// What is proved here, offscreen and without the keyboard focus:
//   - text arrives as posted WM_CHAR (a surrogate pair as two messages) and WM_KEYDOWN, as the harness
//     will send it, and the edit tracks Shift/Ctrl from the messages it sees rather than GetKeyState;
//   - caret movement and Backspace go by DirectWrite cluster, so an emoji is one step;
//   - selection, Shift+Home/End, Ctrl+A, cut/copy/paste through a clipboard seam (nothing touches the
//     real clipboard: a fake is injected, as the harness will do);
//   - undo groups a typing run, a deleting run and a replacement separately, with redo;
//   - an IME composition is inserted at the caret, drawn underlined in its own colour, committed in
//     place, and the candidate window is told where the caret is (ImmSetCandidateWindow);
//   - the caret stays visible: the line scrolls horizontally when text outruns the box;
//   - a PNG of three edits (selection + caret, live composition, placeholder) through the S4 path.
#include <windows.h>

#include <d2d1_1.h>
#include <d3d11.h>
#include <dwrite.h>
#include <dxgi1_2.h>
#include <imm.h>
#include <wincodec.h>
#include <wrl/client.h>

#include <algorithm>
#include <cstdio>
#include <functional>
#include <string>
#include <vector>

using Microsoft::WRL::ComPtr;

namespace {

int failures = 0;

void check(bool ok, const char* what) {
    if (!ok) {
        ++failures;
        std::printf("S1 FAIL: %s\n", what);
    }
}

std::string narrow(const std::wstring& s) {
    std::string out;
    for (wchar_t c : s) {
        out += c < 0x80 ? static_cast<char>(c) : '?';
    }
    return out;
}

void check_text(const std::wstring& actual, const std::wstring& expected, const char* what) {
    if (actual != expected) {
        ++failures;
        std::printf("S1 FAIL: %s: expected \"%s\" got \"%s\"\n", what, narrow(expected).c_str(),
                    narrow(actual).c_str());
    }
}

// The clipboard seam. The app installs the system pair; a harness installs a fake, so a scripted run
// never reads or writes what the user has on the clipboard.
struct Clipboard {
    std::function<std::wstring()> read;
    std::function<void(const std::wstring&)> write;
};

std::wstring read_system_clipboard(HWND owner) {
    std::wstring text;
    if (!OpenClipboard(owner)) {
        return text;
    }
    if (const HANDLE h = GetClipboardData(CF_UNICODETEXT)) {
        if (const auto* p = static_cast<const wchar_t*>(GlobalLock(h))) {
            text = p;
            GlobalUnlock(h);
        }
    }
    CloseClipboard();
    return text;
}

void write_system_clipboard(HWND owner, const std::wstring& text) {
    if (!OpenClipboard(owner)) {
        return;
    }
    EmptyClipboard();
    const size_t bytes = (text.size() + 1) * sizeof(wchar_t);
    if (const HGLOBAL h = GlobalAlloc(GMEM_MOVEABLE, bytes)) {
        if (void* p = GlobalLock(h)) {
            memcpy(p, text.c_str(), bytes);
            GlobalUnlock(h);
            SetClipboardData(CF_UNICODETEXT, h);
        }
    }
    CloseClipboard();
}

enum class EditKind { None, Typing, Deleting, Other };

struct Snapshot {
    std::wstring text;
    size_t anchor = 0;
    size_t caret = 0;
};

struct EditBrushes {
    ID2D1Brush* background;
    ID2D1Brush* border;
    ID2D1Brush* text;
    ID2D1Brush* placeholder;
    ID2D1Brush* selection;
    ID2D1Brush* caret;
    ID2D1Brush* composition;
};

class Edit {
public:
    Edit(IDWriteFactory* dwrite, IDWriteTextFormat* format, Clipboard clipboard)
        : dwrite_(dwrite), format_(format), clipboard_(std::move(clipboard)) {}

    void set_bounds(D2D1_RECT_F bounds) { bounds_ = bounds; }
    void set_placeholder(std::wstring text) { placeholder_ = std::move(text); }

    const std::wstring& text() const { return text_; }
    size_t caret() const { return caret_; }
    size_t anchor() const { return anchor_; }
    bool composing() const { return composing_; }
    float scroll_x() const { return scroll_x_; }

    std::wstring selected_text() const {
        const auto [lo, hi] = selection();
        return text_.substr(lo, hi - lo);
    }

    void select(size_t anchor, size_t caret) {
        anchor_ = std::min(anchor, text_.size());
        caret_ = std::min(caret, text_.size());
        last_kind_ = EditKind::None;
        keep_caret_visible();
    }

    // WM_CHAR. A supplementary character arrives as two messages; the high half waits for its low half.
    void on_char(wchar_t c) {
        if (composing_) {
            return;  // the IME owns the keyboard until it commits or cancels
        }
        if (c == L'\b') {
            backspace();
            return;
        }
        if (c < 0x20 || c == 0x7f) {
            return;  // Ctrl+letter and Tab/Enter arrive here too; the key handler already dealt with them
        }
        if (IS_HIGH_SURROGATE(c)) {
            pending_high_ = c;
            return;
        }
        std::wstring s;
        if (IS_LOW_SURROGATE(c) && pending_high_) {
            s.push_back(pending_high_);
        }
        pending_high_ = 0;
        s.push_back(c);
        insert(s, EditKind::Typing);
    }

    // WM_KEYDOWN / WM_KEYUP. Modifiers are tracked from the messages so a posted key sequence behaves
    // the same as a pressed one; the real app can seed this from GetKeyState on focus.
    void on_key_up(UINT vk) {
        if (vk == VK_SHIFT) shift_ = false;
        if (vk == VK_CONTROL) ctrl_ = false;
    }

    bool on_key_down(UINT vk) {
        if (vk == VK_SHIFT) {
            shift_ = true;
            return true;
        }
        if (vk == VK_CONTROL) {
            ctrl_ = true;
            return true;
        }
        if (composing_) {
            return false;
        }
        switch (vk) {
            case VK_LEFT:
                move(ctrl_ ? word_left(caret_) : prev_cluster(caret_));
                return true;
            case VK_RIGHT:
                move(ctrl_ ? word_right(caret_) : next_cluster(caret_));
                return true;
            case VK_HOME:
                move(0);
                return true;
            case VK_END:
                move(text_.size());
                return true;
            case VK_DELETE:
                del();
                return true;
            case 'A':
                if (ctrl_) {
                    select(0, text_.size());
                    return true;
                }
                return false;
            case 'C':
                if (ctrl_) {
                    clipboard_.write(selected_text());
                    return true;
                }
                return false;
            case 'X':
                if (ctrl_) {
                    clipboard_.write(selected_text());
                    if (caret_ != anchor_) {
                        insert(L"", EditKind::Other);
                    }
                    return true;
                }
                return false;
            case 'V':
                if (ctrl_) {
                    paste();
                    return true;
                }
                return false;
            case 'Z':
                if (ctrl_) {
                    undo();
                    return true;
                }
                return false;
            case 'Y':
                if (ctrl_) {
                    redo();
                    return true;
                }
                return false;
            default:
                return false;
        }
    }

    // A single line: anything the clipboard carries as a line break becomes a space.
    void paste() {
        std::wstring s = clipboard_.read();
        std::wstring flat;
        for (size_t i = 0; i < s.size(); ++i) {
            if (s[i] == L'\r') {
                if (i + 1 < s.size() && s[i + 1] == L'\n') ++i;
                flat.push_back(L' ');
            } else if (s[i] == L'\n') {
                flat.push_back(L' ');
            } else {
                flat.push_back(s[i]);
            }
        }
        if (!flat.empty()) {
            insert(flat, EditKind::Other);
        }
    }

    void undo() { step(undo_, redo_); }
    void redo() { step(redo_, undo_); }

    // IME. The composition lives in the display text only; `text_` changes when it commits.
    void set_composition(const std::wstring& composition, size_t cursor) {
        if (!composing_) {
            if (caret_ != anchor_) {
                insert(L"", EditKind::Other);  // the IME replaces the selection, as a Windows edit does
            }
            composing_ = true;
        }
        composition_ = composition;
        composition_cursor_ = std::min(cursor, composition.size());
        dirty_ = true;
        keep_caret_visible();
    }

    void commit_composition(const std::wstring& result) {
        composing_ = false;
        composition_.clear();
        composition_cursor_ = 0;
        if (!result.empty()) {
            insert(result, EditKind::Other);
        }
        dirty_ = true;
    }

    void end_composition() {
        composing_ = false;
        composition_.clear();
        composition_cursor_ = 0;
        dirty_ = true;
        keep_caret_visible();
    }

    // Where the caret is, in the window's client pixels, for ImmSetCandidateWindow and the caret itself.
    D2D1_RECT_F caret_rect() {
        ensure_layout();
        float x = 0, y = 0;
        DWRITE_HIT_TEST_METRICS m{};
        layout_->HitTestTextPosition(static_cast<UINT32>(display_caret()), FALSE, &x, &y, &m);
        const float left = bounds_.left + kPad - scroll_x_ + x;
        const float top = bounds_.top + kPad + y;
        return D2D1::RectF(left, top, left + 1.0f, top + m.height);
    }

    void paint(ID2D1DeviceContext* ctx, const EditBrushes& b) {
        ensure_layout();
        ctx->FillRectangle(bounds_, b.background);
        ctx->DrawRectangle(D2D1::RectF(bounds_.left + 0.5f, bounds_.top + 0.5f, bounds_.right - 0.5f, bounds_.bottom - 0.5f),
                           b.border, 1.0f);
        const D2D1_RECT_F inner = D2D1::RectF(bounds_.left + kPad, bounds_.top, bounds_.right - kPad, bounds_.bottom);
        ctx->PushAxisAlignedClip(inner, D2D1_ANTIALIAS_MODE_ALIASED);
        const D2D1_POINT_2F origin = D2D1::Point2F(bounds_.left + kPad - scroll_x_, bounds_.top + kPad);

        if (display_text().empty() && !placeholder_.empty()) {
            ctx->DrawTextW(placeholder_.c_str(), static_cast<UINT32>(placeholder_.size()), format_,
                           D2D1::RectF(origin.x, origin.y, inner.right, bounds_.bottom), b.placeholder);
        }

        const auto [lo, hi] = selection();
        if (hi > lo && !composing_) {
            for (const auto& m : range_metrics(lo, hi - lo)) {
                ctx->FillRectangle(D2D1::RectF(origin.x + m.left, origin.y + m.top, origin.x + m.left + m.width,
                                               origin.y + m.top + m.height),
                                   b.selection);
            }
        }
        if (composing_) {
            const DWRITE_TEXT_RANGE range{static_cast<UINT32>(caret_), static_cast<UINT32>(composition_.size())};
            layout_->SetUnderline(TRUE, range);
            layout_->SetDrawingEffect(b.composition, range);
        }
        ctx->DrawTextLayout(origin, layout_.Get(), b.text);

        const D2D1_RECT_F caret = caret_rect();
        ctx->FillRectangle(caret, b.caret);
        ctx->PopAxisAlignedClip();
    }

private:
    static constexpr float kPad = 6.0f;

    std::pair<size_t, size_t> selection() const { return {std::min(anchor_, caret_), std::max(anchor_, caret_)}; }

    std::wstring display_text() const {
        if (!composing_) {
            return text_;
        }
        return text_.substr(0, caret_) + composition_ + text_.substr(caret_);
    }

    size_t display_caret() const { return composing_ ? caret_ + composition_cursor_ : caret_; }

    void ensure_layout() {
        if (layout_ && !dirty_) {
            return;
        }
        const std::wstring shown = display_text();
        layout_.Reset();
        dwrite_->CreateTextLayout(shown.c_str(), static_cast<UINT32>(shown.size()), format_, 1.0e6f,
                                  bounds_.bottom - bounds_.top, &layout_);
        dirty_ = false;
    }

    std::vector<DWRITE_HIT_TEST_METRICS> range_metrics(size_t start, size_t length) {
        ensure_layout();
        UINT32 count = 0;
        layout_->HitTestTextRange(static_cast<UINT32>(start), static_cast<UINT32>(length), 0, 0, nullptr, 0, &count);
        std::vector<DWRITE_HIT_TEST_METRICS> out(count);
        if (count) {
            layout_->HitTestTextRange(static_cast<UINT32>(start), static_cast<UINT32>(length), 0, 0, out.data(), count,
                                      &count);
        }
        return out;
    }

    // Cluster boundaries of the committed text: where the caret may stop. An emoji, or a base letter
    // with a combining mark, is one cluster.
    std::vector<size_t> boundaries() {
        ensure_layout();
        UINT32 count = 0;
        layout_->GetClusterMetrics(nullptr, 0, &count);
        std::vector<DWRITE_CLUSTER_METRICS> clusters(count);
        if (count) {
            layout_->GetClusterMetrics(clusters.data(), count, &count);
        }
        std::vector<size_t> out{0};
        size_t pos = 0;
        for (const auto& c : clusters) {
            pos += c.length;
            out.push_back(pos);
        }
        return out;
    }

    size_t prev_cluster(size_t pos) {
        size_t best = 0;
        for (size_t b : boundaries()) {
            if (b < pos) best = b;
        }
        return best;
    }

    size_t next_cluster(size_t pos) {
        for (size_t b : boundaries()) {
            if (b > pos) return b;
        }
        return text_.size();
    }

    size_t word_left(size_t pos) const {
        while (pos > 0 && text_[pos - 1] == L' ') --pos;
        while (pos > 0 && text_[pos - 1] != L' ') --pos;
        return pos;
    }

    size_t word_right(size_t pos) const {
        while (pos < text_.size() && text_[pos] != L' ') ++pos;
        while (pos < text_.size() && text_[pos] == L' ') ++pos;
        return pos;
    }

    void move(size_t to) {
        caret_ = to;
        if (!shift_) anchor_ = to;
        last_kind_ = EditKind::None;
        keep_caret_visible();
    }

    void backspace() {
        if (caret_ == anchor_) {
            if (caret_ == 0) return;
            anchor_ = prev_cluster(caret_);
        }
        insert(L"", EditKind::Deleting);
    }

    void del() {
        if (caret_ == anchor_) {
            if (caret_ == text_.size()) return;
            anchor_ = next_cluster(caret_);
        }
        insert(L"", EditKind::Deleting);
    }

    // Every change goes through here: replace the selection with `s`, grouping for undo by kind.
    void insert(const std::wstring& s, EditKind kind) {
        if (kind != last_kind_ || kind == EditKind::Other) {
            undo_.push_back({text_, anchor_, caret_});
            redo_.clear();
        }
        last_kind_ = kind;
        const auto [lo, hi] = selection();
        text_.replace(lo, hi - lo, s);
        caret_ = anchor_ = lo + s.size();
        dirty_ = true;
        keep_caret_visible();
    }

    void step(std::vector<Snapshot>& from, std::vector<Snapshot>& to) {
        if (from.empty()) return;
        to.push_back({text_, anchor_, caret_});
        const Snapshot s = from.back();
        from.pop_back();
        text_ = s.text;
        anchor_ = s.anchor;
        caret_ = s.caret;
        last_kind_ = EditKind::None;
        dirty_ = true;
        keep_caret_visible();
    }

    void keep_caret_visible() {
        ensure_layout();
        DWRITE_TEXT_METRICS tm{};
        layout_->GetMetrics(&tm);
        const float inner = (bounds_.right - bounds_.left) - 2 * kPad;
        if (tm.widthIncludingTrailingWhitespace <= inner) {
            scroll_x_ = 0;
            return;
        }
        float x = 0, y = 0;
        DWRITE_HIT_TEST_METRICS m{};
        layout_->HitTestTextPosition(static_cast<UINT32>(display_caret()), FALSE, &x, &y, &m);
        if (x - scroll_x_ > inner - 1.0f) {
            scroll_x_ = x - (inner - 1.0f);
        } else if (x - scroll_x_ < 0) {
            scroll_x_ = x;
        }
        scroll_x_ = std::min(scroll_x_, tm.widthIncludingTrailingWhitespace - inner + 1.0f);
        scroll_x_ = std::max(scroll_x_, 0.0f);
    }

    IDWriteFactory* dwrite_;
    IDWriteTextFormat* format_;
    Clipboard clipboard_;
    D2D1_RECT_F bounds_{};
    std::wstring placeholder_;
    std::wstring text_;
    size_t anchor_ = 0, caret_ = 0;
    bool shift_ = false, ctrl_ = false;
    wchar_t pending_high_ = 0;
    bool composing_ = false;
    std::wstring composition_;
    size_t composition_cursor_ = 0;
    EditKind last_kind_ = EditKind::None;
    std::vector<Snapshot> undo_, redo_;
    ComPtr<IDWriteTextLayout> layout_;
    bool dirty_ = true;
    float scroll_x_ = 0;
};

// The window procedure the real edit will sit behind. The IME messages are the part that cannot be
// exercised without an input method running, so they are kept thin: each one is one call into Edit.
struct Host {
    Edit* edit = nullptr;
    HWND hwnd = nullptr;
    int ime_positioned = 0;

    void position_ime() {
        const HIMC himc = ImmGetContext(hwnd);
        if (!himc) return;
        const D2D1_RECT_F c = edit->caret_rect();
        COMPOSITIONFORM cf{};
        cf.dwStyle = CFS_POINT;
        cf.ptCurrentPos = {static_cast<LONG>(c.left), static_cast<LONG>(c.top)};
        const BOOL a = ImmSetCompositionWindow(himc, &cf);
        CANDIDATEFORM cand{};
        cand.dwStyle = CFS_EXCLUDE;
        cand.ptCurrentPos = cf.ptCurrentPos;
        cand.rcArea = {static_cast<LONG>(c.left), static_cast<LONG>(c.top), static_cast<LONG>(c.right),
                       static_cast<LONG>(c.bottom)};
        const BOOL b = ImmSetCandidateWindow(himc, &cand);
        ImmReleaseContext(hwnd, himc);
        if (a && b) ++ime_positioned;
    }

    std::wstring composition_string(HIMC himc, DWORD what) {
        const LONG bytes = ImmGetCompositionStringW(himc, what, nullptr, 0);
        if (bytes <= 0) return {};
        std::wstring s(static_cast<size_t>(bytes) / sizeof(wchar_t), L'\0');
        ImmGetCompositionStringW(himc, what, s.data(), static_cast<DWORD>(bytes));
        return s;
    }
};

LRESULT CALLBACK wnd_proc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
    auto* host = reinterpret_cast<Host*>(GetWindowLongPtrW(hwnd, GWLP_USERDATA));
    if (!host || !host->edit) {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    Edit& edit = *host->edit;
    switch (msg) {
        case WM_CHAR:
            edit.on_char(static_cast<wchar_t>(wp));
            host->position_ime();
            return 0;
        case WM_KEYDOWN:
            if (edit.on_key_down(static_cast<UINT>(wp))) {
                host->position_ime();
                return 0;
            }
            break;
        case WM_KEYUP:
            edit.on_key_up(static_cast<UINT>(wp));
            return 0;
        case WM_IME_SETCONTEXT:
            // We draw the composition ourselves; the IME keeps its candidate list.
            return DefWindowProcW(hwnd, msg, wp, lp & ~static_cast<LPARAM>(ISC_SHOWUICOMPOSITIONWINDOW));
        case WM_IME_STARTCOMPOSITION:
            edit.set_composition(L"", 0);
            host->position_ime();
            return 0;
        case WM_IME_COMPOSITION: {
            const HIMC himc = ImmGetContext(hwnd);
            if (himc) {
                if (lp & GCS_RESULTSTR) {
                    edit.commit_composition(host->composition_string(himc, GCS_RESULTSTR));
                }
                if (lp & GCS_COMPSTR) {
                    const LONG cursor = ImmGetCompositionStringW(himc, GCS_CURSORPOS, nullptr, 0);
                    edit.set_composition(host->composition_string(himc, GCS_COMPSTR),
                                         cursor > 0 ? static_cast<size_t>(cursor) : 0);
                }
                ImmReleaseContext(hwnd, himc);
            }
            host->position_ime();
            return 0;
        }
        case WM_IME_ENDCOMPOSITION:
            edit.end_composition();
            return 0;
        default:
            break;
    }
    return DefWindowProcW(hwnd, msg, wp, lp);
}

void pump(HWND hwnd) {
    MSG m;
    while (PeekMessageW(&m, hwnd, 0, 0, PM_REMOVE)) {
        DispatchMessageW(&m);  // no TranslateMessage: WM_CHAR is posted explicitly, as the harness does
    }
}

void type(HWND hwnd, const std::wstring& s) {
    for (wchar_t c : s) {
        PostMessageW(hwnd, WM_CHAR, c, 0);
    }
    pump(hwnd);
}

void key(HWND hwnd, UINT vk, bool shift = false, bool ctrl = false) {
    if (shift) PostMessageW(hwnd, WM_KEYDOWN, VK_SHIFT, 0);
    if (ctrl) PostMessageW(hwnd, WM_KEYDOWN, VK_CONTROL, 0);
    PostMessageW(hwnd, WM_KEYDOWN, vk, 0);
    PostMessageW(hwnd, WM_KEYUP, vk, 0);
    if (ctrl) PostMessageW(hwnd, WM_KEYUP, VK_CONTROL, 0);
    if (shift) PostMessageW(hwnd, WM_KEYUP, VK_SHIFT, 0);
    pump(hwnd);
}

bool write_png(const std::wstring& path, UINT width, UINT height, std::vector<BYTE>& pixels) {
    ComPtr<IWICImagingFactory> wic;
    if (FAILED(CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&wic)))) {
        return false;
    }
    ComPtr<IWICStream> stream;
    wic->CreateStream(&stream);
    if (FAILED(stream->InitializeFromFilename(path.c_str(), GENERIC_WRITE))) {
        return false;
    }
    ComPtr<IWICBitmapEncoder> encoder;
    wic->CreateEncoder(GUID_ContainerFormatPng, nullptr, &encoder);
    encoder->Initialize(stream.Get(), WICBitmapEncoderNoCache);
    ComPtr<IWICBitmapFrameEncode> frame;
    encoder->CreateNewFrame(&frame, nullptr);
    frame->Initialize(nullptr);
    frame->SetSize(width, height);
    WICPixelFormatGUID pf = GUID_WICPixelFormat32bppPBGRA;
    frame->SetPixelFormat(&pf);
    if (FAILED(frame->WritePixels(height, width * 4, static_cast<UINT>(pixels.size()), pixels.data()))) {
        return false;
    }
    frame->Commit();
    return SUCCEEDED(encoder->Commit());
}

}  // namespace

int wmain(int argc, wchar_t** argv) {
    const std::wstring out = argc > 1 ? argv[1] : L"s1_edit.png";
    const HWND foreground_before = GetForegroundWindow();
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);

    WNDCLASSW wc{};
    wc.lpfnWndProc = wnd_proc;
    wc.hInstance = GetModuleHandleW(nullptr);
    wc.lpszClassName = L"BbSpikeS1";
    RegisterClassW(&wc);
    const HWND hwnd = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, wc.lpszClassName, L"s1",
                                      WS_OVERLAPPEDWINDOW, -32000, -32000, 640, 200, nullptr, nullptr,
                                      wc.hInstance, nullptr);
    check(hwnd != nullptr, "CreateWindowExW");
    ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    EnableWindow(hwnd, FALSE);

    ComPtr<IDWriteFactory> dwrite;
    DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory),
                        reinterpret_cast<IUnknown**>(dwrite.GetAddressOf()));
    ComPtr<IDWriteTextFormat> format;
    dwrite->CreateTextFormat(L"Segoe UI", nullptr, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL,
                             DWRITE_FONT_STRETCH_NORMAL, 14.0f, L"en-us", &format);

    std::wstring fake_clipboard;
    Clipboard fake{[&] { return fake_clipboard; }, [&](const std::wstring& s) { fake_clipboard = s; }};
    // The system pair compiles and is what the app will install; it is not called here.
    (void)&read_system_clipboard;
    (void)&write_system_clipboard;

    Edit edit(dwrite.Get(), format.Get(), fake);
    edit.set_bounds(D2D1::RectF(20, 20, 300, 48));
    edit.set_placeholder(L"Search");
    Host host{&edit, hwnd, 0};
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(&host));

    // 1. Typing through posted WM_CHAR, including a surrogate pair as two messages.
    type(hwnd, L"Hello wörld \U0001F600");
    check_text(edit.text(), L"Hello wörld \U0001F600", "typed text");
    check(edit.caret() == edit.text().size(), "caret at end after typing");

    // 2. Backspace removes the whole emoji: one cluster, two code units.
    type(hwnd, L"\b");
    check_text(edit.text(), L"Hello wörld ", "backspace over a surrogate pair");

    // 3. Left over the emoji again after redo-free retyping, then word movement.
    type(hwnd, L"\U0001F600");
    key(hwnd, VK_LEFT);
    check(edit.caret() == edit.text().size() - 2, "left steps over the pair as one cluster");
    key(hwnd, VK_LEFT, false, true);
    check(edit.caret() == 6, "ctrl+left lands on the start of the word");
    key(hwnd, VK_END);

    // 4. Shift+Home selects everything; a typed letter replaces the selection.
    key(hwnd, VK_HOME, true);
    check(edit.selected_text() == edit.text(), "shift+home selects all");
    type(hwnd, L"X");
    check_text(edit.text(), L"X", "typing replaces the selection");

    // 5. Undo is grouped: the replacement, the retyped emoji, the backspace, the typing run.
    key(hwnd, 'Z', false, true);
    check_text(edit.text(), L"Hello wörld \U0001F600", "undo the replacement");
    check(edit.selected_text() == edit.text(), "undo restores the selection");
    key(hwnd, 'Z', false, true);
    check_text(edit.text(), L"Hello wörld ", "undo the retyped emoji");
    key(hwnd, 'Z', false, true);
    check_text(edit.text(), L"Hello wörld \U0001F600", "undo the backspace");
    key(hwnd, 'Z', false, true);
    check_text(edit.text(), L"", "undo the whole typing run at once");
    key(hwnd, 'Z', false, true);
    check_text(edit.text(), L"", "undo past the beginning is a no-op");
    key(hwnd, 'Y', false, true);
    check_text(edit.text(), L"Hello wörld \U0001F600", "redo the typing run");
    key(hwnd, 'Y', false, true);
    check_text(edit.text(), L"Hello wörld ", "redo the backspace");

    // 6. Cut, copy and paste through the seam; a multi-line paste is flattened to one line.
    key(hwnd, VK_HOME);
    key(hwnd, VK_RIGHT, true, true);
    check(edit.selected_text() == L"Hello ", "ctrl+shift+right selects a word");
    key(hwnd, 'X', false, true);
    check_text(fake_clipboard, L"Hello ", "cut writes the clipboard");
    check_text(edit.text(), L"wörld ", "cut removes the selection");
    key(hwnd, VK_END);
    fake_clipboard = L"a\r\nb\nc";
    key(hwnd, 'V', false, true);
    check_text(edit.text(), L"wörld a b c", "paste flattens line breaks");
    key(hwnd, 'Z', false, true);
    check_text(edit.text(), L"wörld ", "undo the paste in one step");
    key(hwnd, 'Z', false, true);
    check_text(edit.text(), L"Hello wörld ", "undo the cut");
    key(hwnd, 'A', false, true);
    key(hwnd, 'C', false, true);
    check_text(fake_clipboard, L"Hello wörld ", "copy after select all");

    // 7. IME: a composition at the caret, cursor inside it, committed in place.
    key(hwnd, VK_END);
    edit.set_composition(L"にほ", 1);
    check(edit.composing(), "composition started");
    check_text(edit.text(), L"Hello wörld ", "composition does not touch the text");
    host.position_ime();
    check(host.ime_positioned > 0, "ImmSetCompositionWindow/ImmSetCandidateWindow accepted the caret");
    const float caret_mid_composition = edit.caret_rect().left;
    edit.set_composition(L"にほん", 3);
    check(edit.caret_rect().left > caret_mid_composition, "the caret follows the composition cursor");
    edit.commit_composition(L"日本");
    check(!edit.composing(), "composition ended on commit");
    check_text(edit.text(), L"Hello wörld 日本", "committed text replaces the composition");
    key(hwnd, 'Z', false, true);
    check_text(edit.text(), L"Hello wörld ", "a commit is one undo step");
    key(hwnd, 'Y', false, true);

    // 8. The caret stays visible when the line outruns the box.
    key(hwnd, VK_END);
    check(edit.scroll_x() == 0.0f, "short text is not scrolled");
    type(hwnd, L" and then a great deal more text than fits in the box");
    check(edit.scroll_x() > 0.0f, "long text scrolls to keep the caret visible");
    const D2D1_RECT_F c = edit.caret_rect();
    check(c.left >= 20 && c.left <= 300, "the caret is inside the box after scrolling");
    key(hwnd, VK_HOME);
    check(edit.scroll_x() == 0.0f, "home scrolls back to the start");

    // 9. The picture: selection + caret, a live composition, a placeholder.
    key(hwnd, VK_END);
    for (int i = 0; i < 53; ++i) type(hwnd, L"\b");
    key(hwnd, VK_HOME);
    for (int i = 0; i < 6; ++i) key(hwnd, VK_RIGHT);
    for (int i = 0; i < 5; ++i) key(hwnd, VK_RIGHT, true);
    check(edit.selected_text() == L"wörld", "selection for the picture");

    Edit composing(dwrite.Get(), format.Get(), fake);
    composing.set_bounds(D2D1::RectF(20, 70, 300, 98));
    for (wchar_t ch : std::wstring(L"Type ")) composing.on_char(ch);
    composing.set_composition(L"にほんご", 2);

    Edit empty(dwrite.Get(), format.Get(), fake);
    empty.set_bounds(D2D1::RectF(20, 120, 300, 148));
    empty.set_placeholder(L"Search");

    ComPtr<ID3D11Device> d3d;
    check(SUCCEEDED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT, nullptr,
                                      0, D3D11_SDK_VERSION, &d3d, nullptr, nullptr)),
          "D3D11CreateDevice WARP");
    ComPtr<IDXGIDevice> dxgi;
    d3d.As(&dxgi);
    ComPtr<ID2D1Factory1> factory;
    D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, IID_PPV_ARGS(&factory));
    ComPtr<ID2D1Device> device;
    factory->CreateDevice(dxgi.Get(), &device);
    ComPtr<ID2D1DeviceContext> ctx;
    device->CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE, &ctx);
    const UINT width = 640, height = 170;
    const D2D1_PIXEL_FORMAT fmt = D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED);
    ComPtr<ID2D1Bitmap1> target, staging;
    ctx->CreateBitmap(D2D1::SizeU(width, height), nullptr, 0,
                      D2D1::BitmapProperties1(D2D1_BITMAP_OPTIONS_TARGET, fmt, 96.0f, 96.0f), &target);
    ctx->CreateBitmap(D2D1::SizeU(width, height), nullptr, 0,
                      D2D1::BitmapProperties1(D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW, fmt, 96.0f,
                                              96.0f),
                      &staging);
    check(target && staging, "bitmaps");

    ComPtr<ID2D1SolidColorBrush> bg, border, text, placeholder, selection, caret, composition, page;
    ctx->CreateSolidColorBrush(D2D1::ColorF(0.16f, 0.16f, 0.16f), &bg);
    ctx->CreateSolidColorBrush(D2D1::ColorF(0.45f, 0.45f, 0.45f), &border);
    ctx->CreateSolidColorBrush(D2D1::ColorF(0.90f, 0.90f, 0.90f), &text);
    ctx->CreateSolidColorBrush(D2D1::ColorF(0.55f, 0.55f, 0.55f), &placeholder);
    ctx->CreateSolidColorBrush(D2D1::ColorF(0.20f, 0.45f, 0.80f), &selection);
    ctx->CreateSolidColorBrush(D2D1::ColorF(1.0f, 1.0f, 1.0f), &caret);
    ctx->CreateSolidColorBrush(D2D1::ColorF(0.95f, 0.75f, 0.30f), &composition);
    ctx->CreateSolidColorBrush(D2D1::ColorF(0.118f, 0.118f, 0.118f), &page);
    const EditBrushes brushes{bg.Get(),    border.Get(), text.Get(),        placeholder.Get(),
                              selection.Get(), caret.Get(), composition.Get()};

    ctx->SetTarget(target.Get());
    ctx->BeginDraw();
    ctx->Clear(D2D1::ColorF(0.118f, 0.118f, 0.118f));
    edit.paint(ctx.Get(), brushes);
    composing.paint(ctx.Get(), brushes);
    empty.paint(ctx.Get(), brushes);
    ctx->DrawTextW(L"selection + caret", 17, format.Get(), D2D1::RectF(320, 24, 640, 48), placeholder.Get());
    ctx->DrawTextW(L"IME composition (underlined)", 28, format.Get(), D2D1::RectF(320, 74, 640, 98),
                   placeholder.Get());
    ctx->DrawTextW(L"placeholder", 11, format.Get(), D2D1::RectF(320, 124, 640, 148), placeholder.Get());
    check(SUCCEEDED(ctx->EndDraw()), "EndDraw");

    staging->CopyFromBitmap(nullptr, target.Get(), nullptr);
    D2D1_MAPPED_RECT mapped{};
    check(SUCCEEDED(staging->Map(D2D1_MAP_OPTIONS_READ, &mapped)), "Map");
    std::vector<BYTE> pixels(static_cast<size_t>(width) * height * 4);
    for (UINT y = 0; y < height; ++y) {
        memcpy(pixels.data() + static_cast<size_t>(y) * width * 4, mapped.bits + static_cast<size_t>(y) * mapped.pitch,
               static_cast<size_t>(width) * 4);
    }
    staging->Unmap();

    // The selection blue and the composition amber must both be on the page.
    size_t selection_px = 0, composition_px = 0;
    for (size_t i = 0; i < pixels.size(); i += 4) {
        const BYTE b = pixels[i], g = pixels[i + 1], r = pixels[i + 2];
        if (r < 80 && g > 90 && g < 140 && b > 180) ++selection_px;
        if (r > 200 && g > 160 && g < 210 && b < 110) ++composition_px;
    }
    check(selection_px > 100, "selection highlight is visible");
    check(composition_px > 20, "composition underline is visible");

    check(write_png(out, width, height, pixels), "write png");

    const bool foreground_same = GetForegroundWindow() == foreground_before;
    DestroyWindow(hwnd);
    check(foreground_same, "the foreground window changed");

    if (failures) {
        std::printf("S1 FAILED: %d check(s)\n", failures);
        return 1;
    }
    std::printf("S1 OK: typing, clusters, selection, undo/redo, clipboard seam, composition, scrolling -> %ls\n",
                out.c_str());
    return 0;
}
