#include "views/file_drag.h"

#include <shellapi.h>
#include <shlobj.h>

#include <algorithm>
#include <optional>

#include "views/folder_tree.h"
#include "views/main_view.h"
#include "views/ops.h"

namespace bb::views {

using namespace bb::ui;

namespace {

// The private format that says a drag is ours, and the two a target writes back to say what it did.
UINT items_format() {
    static const UINT f = RegisterClipboardFormatW(L"BertBrowser.FileItems");
    return f;
}
UINT preferred_format() {
    static const UINT f = RegisterClipboardFormatW(CFSTR_PREFERREDDROPEFFECT);
    return f;
}
UINT performed_format() {
    static const UINT f = RegisterClipboardFormatW(CFSTR_PERFORMEDDROPEFFECT);
    return f;
}
UINT logical_format() {
    static const UINT f = RegisterClipboardFormatW(CFSTR_LOGICALPERFORMEDDROPEFFECT);
    return f;
}

FORMATETC hglobal_format(UINT cf) {
    FORMATETC f{};
    f.cfFormat = static_cast<CLIPFORMAT>(cf);
    f.dwAspect = DVASPECT_CONTENT;
    f.lindex = -1;
    f.tymed = TYMED_HGLOBAL;
    return f;
}

std::vector<uint8_t> dword_bytes(DWORD v) {
    std::vector<uint8_t> out(sizeof(v));
    memcpy(out.data(), &v, sizeof(v));
    return out;
}

// Wide strings, each ending in a NUL, and one more to end the list.
std::vector<uint8_t> list_of(const std::vector<std::wstring>& paths, size_t header) {
    std::vector<wchar_t> chars;
    for (const auto& p : paths) {
        chars.insert(chars.end(), p.begin(), p.end());
        chars.push_back(L'\0');
    }
    chars.push_back(L'\0');
    std::vector<uint8_t> out(header + chars.size() * sizeof(wchar_t));
    memcpy(out.data() + header, chars.data(), chars.size() * sizeof(wchar_t));
    return out;
}

// What a drag carries, held in memory. It also keeps what a target writes back (`Performed DropEffect`),
// which is how the source learns whether a "move" really moved.
class DataObject : public IDataObject {
public:
    DataObject() = default;

    void put(UINT cf, std::vector<uint8_t> bytes) {
        for (auto& e : items_) {
            if (e.first.cfFormat == cf) {
                e.second = std::move(bytes);
                return;
            }
        }
        items_.push_back({hglobal_format(cf), std::move(bytes)});
    }
    std::optional<DWORD> dword(UINT cf) const {
        for (const auto& e : items_) {
            if (e.first.cfFormat == cf && e.second.size() >= sizeof(DWORD)) {
                DWORD v;
                memcpy(&v, e.second.data(), sizeof(v));
                return v;
            }
        }
        return std::nullopt;
    }

    // IUnknown
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void** out) override {
        if (riid == IID_IUnknown || riid == IID_IDataObject) {
            *out = static_cast<IDataObject*>(this);
            AddRef();
            return S_OK;
        }
        *out = nullptr;
        return E_NOINTERFACE;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return static_cast<ULONG>(InterlockedIncrement(&refs_)); }
    ULONG STDMETHODCALLTYPE Release() override {
        const LONG n = InterlockedDecrement(&refs_);
        if (n == 0) delete this;
        return static_cast<ULONG>(n);
    }

    // IDataObject
    HRESULT STDMETHODCALLTYPE GetData(FORMATETC* f, STGMEDIUM* m) override {
        if (!f || !m) return E_INVALIDARG;
        for (const auto& e : items_) {
            if (e.first.cfFormat != f->cfFormat || !(f->tymed & TYMED_HGLOBAL)) continue;
            HGLOBAL h = GlobalAlloc(GMEM_MOVEABLE, e.second.size());
            if (!h) return E_OUTOFMEMORY;
            memcpy(GlobalLock(h), e.second.data(), e.second.size());
            GlobalUnlock(h);
            m->tymed = TYMED_HGLOBAL;
            m->hGlobal = h;
            m->pUnkForRelease = nullptr;
            return S_OK;
        }
        return DV_E_FORMATETC;
    }
    HRESULT STDMETHODCALLTYPE GetDataHere(FORMATETC*, STGMEDIUM*) override { return E_NOTIMPL; }
    HRESULT STDMETHODCALLTYPE QueryGetData(FORMATETC* f) override {
        if (!f) return E_INVALIDARG;
        for (const auto& e : items_) {
            if (e.first.cfFormat == f->cfFormat && (f->tymed & TYMED_HGLOBAL)) return S_OK;
        }
        return DV_E_FORMATETC;
    }
    HRESULT STDMETHODCALLTYPE GetCanonicalFormatEtc(FORMATETC*, FORMATETC* out) override {
        out->ptd = nullptr;
        return E_NOTIMPL;
    }
    HRESULT STDMETHODCALLTYPE SetData(FORMATETC* f, STGMEDIUM* m, BOOL release) override {
        if (!f || !m || m->tymed != TYMED_HGLOBAL || !m->hGlobal) return E_INVALIDARG;
        const SIZE_T n = GlobalSize(m->hGlobal);
        std::vector<uint8_t> bytes(n);
        if (void* p = GlobalLock(m->hGlobal)) {
            memcpy(bytes.data(), p, n);
            GlobalUnlock(m->hGlobal);
        }
        put(f->cfFormat, std::move(bytes));
        if (release) ReleaseStgMedium(m);
        return S_OK;
    }
    HRESULT STDMETHODCALLTYPE EnumFormatEtc(DWORD dir, IEnumFORMATETC** out) override {
        if (dir != DATADIR_GET) return E_NOTIMPL;
        std::vector<FORMATETC> formats;
        for (const auto& e : items_) formats.push_back(e.first);
        return SHCreateStdEnumFmtEtc(static_cast<UINT>(formats.size()), formats.data(), out);
    }
    HRESULT STDMETHODCALLTYPE DAdvise(FORMATETC*, DWORD, IAdviseSink*, DWORD*) override { return OLE_E_ADVISENOTSUPPORTED; }
    HRESULT STDMETHODCALLTYPE DUnadvise(DWORD) override { return OLE_E_ADVISENOTSUPPORTED; }
    HRESULT STDMETHODCALLTYPE EnumDAdvise(IEnumSTATDATA**) override { return OLE_E_ADVISENOTSUPPORTED; }

private:
    LONG refs_ = 1;
    std::vector<std::pair<FORMATETC, std::vector<uint8_t>>> items_;
};

// Ends the drag the way a drag ends: Esc cancels, releasing the button that started it drops.
class Source : public IDropSource {
public:
    explicit Source(bool right) : right_(right) {}
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void** out) override {
        if (riid == IID_IUnknown || riid == IID_IDropSource) {
            *out = static_cast<IDropSource*>(this);
            AddRef();
            return S_OK;
        }
        *out = nullptr;
        return E_NOINTERFACE;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return static_cast<ULONG>(InterlockedIncrement(&refs_)); }
    ULONG STDMETHODCALLTYPE Release() override {
        const LONG n = InterlockedDecrement(&refs_);
        if (n == 0) delete this;
        return static_cast<ULONG>(n);
    }
    HRESULT STDMETHODCALLTYPE QueryContinueDrag(BOOL escape, DWORD keys) override {
        if (escape) return DRAGDROP_S_CANCEL;
        if (!(keys & (right_ ? MK_RBUTTON : MK_LBUTTON))) return DRAGDROP_S_DROP;
        return S_OK;
    }
    HRESULT STDMETHODCALLTYPE GiveFeedback(DWORD) override { return DRAGDROP_S_USEDEFAULTCURSORS; }

private:
    LONG refs_ = 1;
    bool right_;
};

struct Payload {
    std::vector<std::wstring> paths;
    bool in_app = false;
};

std::vector<std::wstring> split_nul(const wchar_t* chars, size_t max_chars) {
    std::vector<std::wstring> out;
    size_t i = 0;
    while (i < max_chars && chars[i]) {
        const size_t len = wcsnlen(chars + i, max_chars - i);
        out.emplace_back(chars + i, len);
        i += len + 1;
    }
    return out;
}

std::optional<Payload> read_payload(IDataObject* obj) {
    FORMATETC ours = hglobal_format(items_format());
    STGMEDIUM m{};
    if (SUCCEEDED(obj->GetData(&ours, &m))) {
        Payload p;
        p.in_app = true;
        if (void* data = GlobalLock(m.hGlobal)) {
            p.paths = split_nul(static_cast<const wchar_t*>(data), GlobalSize(m.hGlobal) / sizeof(wchar_t));
            GlobalUnlock(m.hGlobal);
        }
        ReleaseStgMedium(&m);
        if (!p.paths.empty()) return p;
    }
    FORMATETC drop = hglobal_format(CF_HDROP);
    if (SUCCEEDED(obj->GetData(&drop, &m))) {
        Payload p;
        if (const HDROP h = static_cast<HDROP>(GlobalLock(m.hGlobal))) {
            const UINT n = DragQueryFileW(h, 0xFFFFFFFF, nullptr, 0);
            for (UINT i = 0; i < n; ++i) {
                wchar_t path[MAX_PATH * 4];
                if (DragQueryFileW(h, i, path, static_cast<UINT>(std::size(path)))) p.paths.emplace_back(path);
            }
            GlobalUnlock(m.hGlobal);
        }
        ReleaseStgMedium(&m);
        if (!p.paths.empty()) return p;
    }
    return std::nullopt;
}

std::string json_array(const std::vector<std::wstring>& items) { return ops::qa(items); }

// A file list as CF_HDROP carries it: the DROPFILES header, then the paths.
std::vector<uint8_t> hdrop_bytes(const std::vector<std::wstring>& paths) {
    DROPFILES df{};
    df.pFiles = sizeof(df);
    df.fWide = TRUE;
    std::vector<uint8_t> bytes = list_of(paths, sizeof(df));
    memcpy(bytes.data(), &df, sizeof(df));
    return bytes;
}

}  // namespace

// ---- the clipboard -------------------------------------------------------------------------------------------

namespace file_clipboard {

bool put(const std::vector<std::wstring>& paths, bool cut) {
    auto* data = new DataObject();
    data->put(CF_HDROP, hdrop_bytes(paths));
    data->put(preferred_format(), dword_bytes(cut ? DROPEFFECT_MOVE : DROPEFFECT_COPY));
    bool ok = SUCCEEDED(OleSetClipboard(data));
    // Rendered onto the clipboard now, so the files stay there after this window has gone.
    if (ok) ok = SUCCEEDED(OleFlushClipboard());
    data->Release();
    return ok;
}

std::optional<ClipboardFiles> get() {
    IDataObject* obj = nullptr;
    if (FAILED(OleGetClipboard(&obj)) || !obj) return std::nullopt;
    std::optional<ClipboardFiles> out;
    if (std::optional<Payload> p = read_payload(obj)) {
        ClipboardFiles files;
        files.paths = std::move(p->paths);
        FORMATETC f = hglobal_format(preferred_format());
        STGMEDIUM m{};
        if (SUCCEEDED(obj->GetData(&f, &m))) {
            if (GlobalSize(m.hGlobal) >= sizeof(DWORD)) {
                if (const auto* effect = static_cast<const DWORD*>(GlobalLock(m.hGlobal))) {
                    files.cut = (*effect & DROPEFFECT_MOVE) != 0;
                    GlobalUnlock(m.hGlobal);
                }
            }
            ReleaseStgMedium(&m);
        }
        out = std::move(files);
    }
    obj->Release();
    return out;
}

bool has_files() { return IsClipboardFormatAvailable(CF_HDROP) != FALSE; }

void clear() {
    if (!OpenClipboard(nullptr)) return;
    EmptyClipboard();
    CloseClipboard();
}

}  // namespace file_clipboard

uint64_t transfer_into(core::Core& core, const std::vector<std::wstring>& sources, const std::wstring& destination,
                       const std::string& verb) {
    try {
        if (verb == "shortcut") {
            ops::run(core, "shortcut", "{\"sources\":" + json_array(sources) + ",\"directory\":" + ops::q(destination) + "}");
            return 0;
        }
        // Into a container is a different operation: the archive is rewritten with the files added.
        const json::Value inside = ops::query(core, "archive-of", core::utf8(destination));
        if (!inside.is_null()) {
            ops::run(core, "archive-add",
                     "{\"archive\":" + ops::q(core::wide(inside["archive"].str())) + ",\"entryDir\":" + ops::q(core::wide(inside["entry"].str())) +
                         ",\"sources\":" + json_array(sources) + "}");
            return 0;
        }
        std::vector<BbStr> views;
        for (const auto& p : sources) views.push_back(core::s(p));
        uint64_t op = 0;
        core::check((verb == "copy" ? bb_op_copy : bb_op_move)(core.app(), views.data(), static_cast<uint32_t>(views.size()), core::s(destination), &op),
                    "transfer");
        return op;
    } catch (const std::exception&) {
        // The core has already said why on the status line.
        return 0;
    }
}

// ---- the window as a drop target -------------------------------------------------------------------------------

class FileDrag::Target : public IDropTarget {
public:
    explicit Target(FileDrag& owner) : owner_(owner) {}

    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void** out) override {
        if (riid == IID_IUnknown || riid == IID_IDropTarget) {
            *out = static_cast<IDropTarget*>(this);
            AddRef();
            return S_OK;
        }
        *out = nullptr;
        return E_NOINTERFACE;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return static_cast<ULONG>(InterlockedIncrement(&refs_)); }
    ULONG STDMETHODCALLTYPE Release() override {
        const LONG n = InterlockedDecrement(&refs_);
        if (n == 0) delete this;
        return static_cast<ULONG>(n);
    }

    HRESULT STDMETHODCALLTYPE DragEnter(IDataObject* obj, DWORD keys, POINTL pt, DWORD* effect) override {
        payload_ = read_payload(obj);
        cached_destination_.clear();
        return DragOver(keys, pt, effect);
    }

    HRESULT STDMETHODCALLTYPE DragOver(DWORD keys, POINTL pt, DWORD* effect) override {
        const DWORD allowed = *effect;
        *effect = DROPEFFECT_NONE;
        const auto site = site_at(pt);
        if (!payload_ || !site) {
            clear_highlight();
            return S_OK;
        }
        const json::Value decision = decide(keys, allowed);
        if (!decision["acceptable"].boolean()) {
            clear_highlight();
            return S_OK;
        }
        const std::string verb = decision["verb"].str();
        if (!allowed_here(site->destination, verb)) {
            clear_highlight();
            return S_OK;
        }
        // While hovering the cursor shows the verb either way; "report nothing" is the drop's rule.
        *effect = verb == "copy" ? DROPEFFECT_COPY : DROPEFFECT_MOVE;
        highlight(*site);
        return S_OK;
    }

    HRESULT STDMETHODCALLTYPE DragLeave() override {
        clear_highlight();
        payload_.reset();
        return S_OK;
    }

    HRESULT STDMETHODCALLTYPE Drop(IDataObject* obj, DWORD keys, POINTL pt, DWORD* effect) override {
        const DWORD allowed = *effect;
        clear_highlight();
        cached_destination_.clear();
        *effect = DROPEFFECT_NONE;
        std::optional<Payload> payload = read_payload(obj);
        const auto site = site_at(pt);
        if (!payload || !site) return S_OK;
        payload_ = payload;
        const json::Value decision = decide(keys, allowed);
        const std::string verb = decision["verb"].str();
        if (payload->in_app) {
            // Ours: the drag source must not also act on it. Both of the next two lines say so
            // independently - the session flag, and an effect of None for DoDragDrop to return.
            if (owner_.session_) owner_.session_->handled_in_app = true;
            if (owner_.session_ && owner_.session_->right_button) {
                // Asked here, but only *opened* once this call has returned: DoDragDrop's modal loop still
                // owns the mouse, and a menu opened under it never sees the click meant to choose from it.
                POINT screen{pt.x, pt.y};
                ScreenToClient(owner_.main_.window().hwnd(), &screen);
                const Point at = owner_.main_.window().to_dip(screen.x, screen.y);
                const std::vector<std::wstring> sources = payload->paths;
                const std::wstring destination = site->destination;
                FileDrag* self = &owner_;
                owner_.main_.defer([self, sources, destination, at] {
                    self->main_.show_context_menu(self->verb_menu(sources, destination), at);
                });
                return S_OK;
            }
        } else {
            // The opposite obligation for a foreign drag: the source is waiting to be told what happened,
            // and a move it is not told about leaves its copy behind.
            *effect = decision["report"].i64() == 2 ? DROPEFFECT_MOVE : DROPEFFECT_COPY;
        }
        owner_.perform(payload->paths, site->destination, verb);
        return S_OK;
    }

private:
    struct Site {
        std::wstring destination;
        ListView* list = nullptr;
        TileView* tiles = nullptr;  // in the thumbnail view the folder is a tile, not a list row
        size_t row = ListView::npos;
        TreeView* tree = nullptr;   // a folder in the tree, outlined while the drag is over it
        uint64_t tree_id = 0;
    };

    // Where a drop at this point would land: a folder row, else the folder on show (not a search result).
    std::optional<Site> site_at(POINTL pt) {
        MainView& main = owner_.main_;
        POINT screen{pt.x, pt.y};
        ScreenToClient(main.window().hwnd(), &screen);
        const Point p = main.window().to_dip(screen.x, screen.y);
        Widget* w = main.window().root().hit_test(p);
        for (; w; w = w->parent()) {
            if (auto* tree = dynamic_cast<TreeView*>(w)) {
                if (tree != main.tree_view()) return std::nullopt;
                const size_t row = tree->row_at(p);
                if (row == TreeView::npos) return std::nullopt;
                const std::wstring path = main.folder_tree().path_of(tree->id_at_row(row));
                if (path.empty()) return std::nullopt;
                Site site{path, nullptr, nullptr, ListView::npos};
                site.tree = tree;
                site.tree_id = tree->id_at_row(row);
                return site;
            }
            if (auto* tab = dynamic_cast<TabView*>(w)) {
                ListView* list = tab->list();
                TileView* tiles = tab->tiles();
                Site site;
                size_t row = ListView::npos;
                if (tab->tiles_mode() && tiles && tiles->shown() && tiles->bounds.contains(p)) {
                    site.tiles = tiles;
                    row = tiles->row_at(p);
                } else if (list && list->shown() && list->bounds.contains(p)) {
                    site.list = list;
                    row = list->row_at(p);
                } else {
                    // Only the list (or its tiles) takes a drop: the nav row, the breadcrumb and the side
                    // pane are not places to put files. Spec: FileDragDropController (list.AllowDrop).
                    return std::nullopt;
                }
                BbRow r{};
                auto listing = tab->adapter().listing();
                if (row != ListView::npos && listing && listing->row(row, r) && (r.flags & BB_ROW_DIRECTORY)) {
                    site.destination = core::w(r.full_path);
                    site.row = row;
                    return site;
                }
                const BbTabState st = main.core().tab_state(tab->tab());
                if (st.is_flattened && !st.is_flat_view) return std::nullopt;  // a search result has no "here"
                site.destination = tab->path_text();
                if (site.destination.empty()) return std::nullopt;
                return site;
            }
        }
        return std::nullopt;
    }

    json::Value decide(DWORD keys, DWORD allowed) {
        return ops::query(owner_.main_.core(), "drop-decide",
                          std::string("{\"origin\":\"") + (payload_ && payload_->in_app ? "in-app" : "external") + "\",\"control\":" +
                              ((keys & MK_CONTROL) ? "true" : "false") + ",\"shift\":" + ((keys & MK_SHIFT) ? "true" : "false") +
                              ",\"allowed\":" + std::to_string(allowed) + "}");
    }

    // The hover plan is cached: the sources are fixed for the drag, so only the target and the verb change.
    bool allowed_here(const std::wstring& destination, const std::string& verb) {
        if (cached_destination_ == destination && cached_verb_ == verb) return cached_allowed_;
        cached_destination_ = destination;
        cached_verb_ = verb;
        cached_allowed_ = ops::query(owner_.main_.core(), "drop-allowed",
                                     "{\"sources\":" + json_array(payload_->paths) + ",\"destination\":" + ops::q(destination) +
                                         ",\"verb\":\"" + verb + "\"}")["ok"].boolean();
        return cached_allowed_;
    }

    void highlight(const Site& site) {
        if (site.list == lit_list_ && site.tiles == lit_tiles_ && site.row == lit_row_ && site.tree == lit_tree_ &&
            site.tree_id == lit_tree_id_) {
            return;
        }
        clear_highlight();
        if (site.tree && site.tree_id) {
            site.tree->drop_id = site.tree_id;
            site.tree->invalidate();
            lit_tree_ = site.tree;
            lit_tree_id_ = site.tree_id;
            return;
        }
        if (site.row == ListView::npos) return;
        if (site.list) {
            site.list->drop_row = site.row;
            site.list->invalidate();
        }
        if (site.tiles) {
            site.tiles->drop_row = site.row;
            site.tiles->invalidate();
        }
        lit_list_ = site.list;
        lit_tiles_ = site.tiles;
        lit_row_ = site.row;
    }
    void clear_highlight() {
        if (lit_list_) {
            lit_list_->drop_row = ListView::npos;
            lit_list_->invalidate();
        }
        if (lit_tiles_) {
            lit_tiles_->drop_row = TileView::npos;
            lit_tiles_->invalidate();
        }
        if (lit_tree_) {
            lit_tree_->drop_id = 0;
            lit_tree_->invalidate();
        }
        lit_tree_ = nullptr;
        lit_tree_id_ = 0;
        lit_list_ = nullptr;
        lit_tiles_ = nullptr;
        lit_row_ = ListView::npos;
    }

    LONG refs_ = 1;
    FileDrag& owner_;
    std::optional<Payload> payload_;
    std::wstring cached_destination_;
    std::string cached_verb_;
    bool cached_allowed_ = false;
    ListView* lit_list_ = nullptr;
    TileView* lit_tiles_ = nullptr;
    TreeView* lit_tree_ = nullptr;
    uint64_t lit_tree_id_ = 0;
    size_t lit_row_ = ListView::npos;
};

// ---- the drag source and the verbs -------------------------------------------------------------------------------

FileDrag::FileDrag(MainView& main) : main_(main) {}

FileDrag::~FileDrag() {
    if (registered_) RevokeDragDrop(main_.window().hwnd());
    if (target_) target_->Release();
}

void FileDrag::enable() {
    if (registered_) return;
    target_ = new Target(*this);
    registered_ = SUCCEEDED(RegisterDragDrop(main_.window().hwnd(), target_));
}

void FileDrag::start(uint64_t tab, MouseButton button) {
    const std::vector<std::wstring> paths = main_.core().selection_paths(tab);
    if (paths.empty() || session_) return;
    Session session;
    session.right_button = button == MouseButton::Right;
    session_ = &session;

    auto* data = new DataObject();
    data->put(items_format(), list_of(paths, 0));
    // What makes this a drag source for Explorer, editors, browsers and mail clients.
    data->put(CF_HDROP, hdrop_bytes(paths));
    // Ask for a copy: between two folders on one volume Explorer would otherwise default to a move, and
    // dragging a file into another program should add it there rather than quietly take it out of the
    // folder being browsed. Shift still overrides this.
    data->put(preferred_format(), dword_bytes(DROPEFFECT_COPY));
    auto* source = new Source(session.right_button);

    DWORD effect = DROPEFFECT_NONE;
    const HRESULT hr = DoDragDrop(data, source, DROPEFFECT_COPY | DROPEFFECT_MOVE, &effect);
    if (FAILED(hr)) effect = DROPEFFECT_NONE;

    // Whether the originals are now ours to remove is the one genuinely dangerous question here, so it is
    // decided by a pure, tested rule in the core rather than inline.
    std::string arg = "{\"handledInApp\":" + std::string(session.handled_in_app ? "true" : "false") + ",\"returned\":" + std::to_string(effect);
    if (const auto v = data->dword(logical_format())) arg += ",\"logical\":" + std::to_string(*v);
    if (const auto v = data->dword(performed_format())) arg += ",\"performed\":" + std::to_string(*v);
    arg += "}";
    std::string action = "nothing";
    try {
        action = ops::query(main_.core(), "dragout-decide", arg).str();
    } catch (const std::exception&) {
    }
    source->Release();
    data->Release();
    session_ = nullptr;

    try {
        if (action == "remove") {
            std::vector<BbStr> views;
            for (const auto& p : paths) views.push_back(core::s(p));
            uint64_t op = 0;
            bb_op_delete(main_.core().app(), views.data(), static_cast<uint32_t>(views.size()), BB_DELETE_RECYCLE, &op);
        } else if (action == "refresh") {
            bb_tab_refresh(main_.core().app(), main_.active_tab());
        }
    } catch (const std::exception&) {
    }
}

void FileDrag::perform(const std::vector<std::wstring>& sources, const std::wstring& destination, const std::string& verb) {
    transfer_into(main_.core(), sources, destination, verb);
}

MenuItems FileDrag::verb_menu(const std::vector<std::wstring>& sources, const std::wstring& destination) {
    MenuItems items;
    const json::Value entries = ops::query(main_.core(), "right-drop-menu", "{\"sources\":" + json_array(sources) + ",\"destination\":" + ops::q(destination) + "}");
    for (size_t i = 0; i < entries.size(); ++i) {
        const std::string verb = entries[i]["verb"].str();
        // No icons, deliberately: Explorer's right-drag menu has none, and a wrong picture is the one UI
        // mistake nothing catches - here it would sit on the verb that removes the originals.
        MenuItem m = MenuItem::make(core::wide(entries[i]["label"].str()), [this, sources, destination, verb] { perform(sources, destination, verb); });
        m.enabled = entries[i]["enabled"].boolean();
        items.push_back(std::move(m));
    }
    items.push_back(MenuItem::separator());
    // Dismissing the menu any other way means the same thing, so this needs no handler.
    items.push_back(MenuItem::make(L"Cancel"));
    return items;
}

}  // namespace bb::views
