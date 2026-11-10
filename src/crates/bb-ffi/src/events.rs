//! Events and the sink that delivers them.

use super::*;

// ---- events ----

/// What an event is about. `BbEvent.id` and `a` mean different things per kind; each variant says.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BbEventKind {
    /// A tab's listing was replaced. `id` = tab, `a` = the new listing version. Drop the snapshot you
    /// hold and its text layouts, and ask for a new one.
    BB_EVT_LISTING_CHANGED = 1,
    /// A tab's path, history, flags, status text or selection moved. `id` = tab.
    BB_EVT_TAB_CHANGED = 2,
    /// Panes or tabs were added, removed or activated.
    BB_EVT_LAYOUT_CHANGED = 3,
    /// The settle predicate flipped. `a` = 1 when busy.
    BB_EVT_BUSY_CHANGED = 4,
    /// The undo history gained, lost or moved an entry.
    BB_EVT_UNDO_CHANGED = 5,
    /// An operation finished. `id` = operation, `a` = 1 when it succeeded.
    BB_EVT_OP_FINISHED = 6,
    /// A setting changed. `text` = its key.
    BB_EVT_SETTINGS_CHANGED = 7,
    /// A pane took the focus or changed its tab. `id` = pane.
    BB_EVT_PANE_CHANGED = 8,
    /// A service's state moved; re-read it. `a` = the service: 1 settings page, 2 theme (re-push the
    /// palette), 3 menu bar/toolbars, 4 keymap, 5 palette, 6 Keyboard page, 7 Menus and toolbars
    /// page, 8 saved searches/workspaces/bookmarks, 9 index banner/status, 10 columns, 11 compare,
    /// 12 command availability.
    BB_EVT_MODEL_CHANGED = 9,
    /// A tab shows shell-property columns with cells the host has not read. `id` = tab. Ask with
    /// `bb_props_requests`, answer with `bb_props_supply`.
    BB_EVT_PROPERTIES_NEEDED = 10,
}

/// An event delivered to the sink. Rust owns it; call `bb_event_free` when done. `text` is valid
/// until then.
#[repr(C)]
pub struct BbEvent {
    pub kind: BbEventKind,
    pub id: u64,
    pub a: u64,
    pub b: u64,
    pub text: BbStr,
    pub payload: *const c_void,
}

/// The receiver of events. Called on Rust threads: enqueue and return, do nothing else.
pub type BbEventSink = Option<unsafe extern "C" fn(ctx: *mut c_void, evt: *mut BbEvent)>;

#[repr(C)]
pub(crate) struct EventBox {
    event: BbEvent,
    text: Vec<u16>,
}

pub(crate) fn deliver(
    sink: BbEventSink,
    ctx: usize,
    kind: BbEventKind,
    id: u64,
    a: u64,
    text: &str,
) {
    let Some(sink) = sink else { return };
    let mut boxed = Box::new(EventBox {
        event: BbEvent {
            kind,
            id,
            a,
            b: 0,
            text: BbStr::EMPTY,
            payload: ptr::null(),
        },
        text: text.encode_utf16().collect(),
    });
    boxed.event.text = BbStr::of(&boxed.text);
    let raw = Box::into_raw(boxed);
    // EventBox is repr(C) with `event` first, so a pointer to the box is a pointer to the event.
    // SAFETY: `sink` is the C++ receiver, which only enqueues; it owns `raw` until `bb_event_free`.
    let _ = panic::catch_unwind(AssertUnwindSafe(|| unsafe {
        sink(ctx as *mut c_void, raw.cast::<BbEvent>())
    }));
}

pub(crate) type SinkSlot =
    Arc<Mutex<Option<(unsafe extern "C" fn(*mut c_void, *mut BbEvent), usize)>>>;

pub(crate) fn forward(slot: &SinkSlot, event: AppEvent) {
    let installed = *slot.lock().unwrap_or_else(|p| p.into_inner());
    let Some((sink, ctx)) = installed else { return };
    let sink = Some(sink);
    match event {
        AppEvent::ListingChanged { tab, version } => deliver(
            sink,
            ctx,
            BbEventKind::BB_EVT_LISTING_CHANGED,
            tab.0,
            version,
            "",
        ),
        AppEvent::TabChanged { tab } => {
            deliver(sink, ctx, BbEventKind::BB_EVT_TAB_CHANGED, tab.0, 0, "")
        }
        AppEvent::LayoutChanged => deliver(sink, ctx, BbEventKind::BB_EVT_LAYOUT_CHANGED, 0, 0, ""),
        AppEvent::BusyChanged { busy } => deliver(
            sink,
            ctx,
            BbEventKind::BB_EVT_BUSY_CHANGED,
            0,
            u64::from(busy),
            "",
        ),
        AppEvent::UndoChanged => deliver(sink, ctx, BbEventKind::BB_EVT_UNDO_CHANGED, 0, 0, ""),
        AppEvent::OpFinished { op, ok } => deliver(
            sink,
            ctx,
            BbEventKind::BB_EVT_OP_FINISHED,
            op.0,
            u64::from(ok),
            "",
        ),
        AppEvent::SettingsChanged { key } => {
            deliver(sink, ctx, BbEventKind::BB_EVT_SETTINGS_CHANGED, 0, 0, &key)
        }
        AppEvent::PaneChanged { pane } => {
            deliver(sink, ctx, BbEventKind::BB_EVT_PANE_CHANGED, pane.0, 0, "")
        }
        AppEvent::ModelChanged { model } => deliver(
            sink,
            ctx,
            BbEventKind::BB_EVT_MODEL_CHANGED,
            0,
            u64::from(model as u8),
            "",
        ),
        AppEvent::PropertiesNeeded { tab } => deliver(
            sink,
            ctx,
            BbEventKind::BB_EVT_PROPERTIES_NEEDED,
            tab.0,
            0,
            "",
        ),
    }
}
