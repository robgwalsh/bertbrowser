#ifndef BBCORE_H
#define BBCORE_H

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

// `BbRow.flags` bits.
#define BB_ROW_DIRECTORY 1

#define BB_ROW_HIDDEN 2

#define BB_ROW_MEDIA 4

// A folder size that is a floor, because some folders could not be read.
#define BB_ROW_SIZE_INCOMPLETE 8

// `BbFocusState.flags` bits.
#define BB_FOCUS_SETTINGS_OPEN 1

#define BB_FOCUS_PALETTE_OPEN 2

// A modal dialog is up but the window still receives keys behind it.
#define BB_FOCUS_CONTENT_BLOCKED 4

// The Keyboard page is waiting for a chord.
#define BB_FOCUS_RECORDING 8

// A menu or dropdown has the keyboard.
#define BB_FOCUS_IN_POPUP 16

// What an event is about. `BbEvent.id` and `a` mean different things per kind; each variant says.
typedef enum BbEventKind {
  // A tab's listing was replaced. `id` = tab, `a` = the new listing version. Drop the snapshot you
  // hold and its text layouts, and ask for a new one.
  BB_EVT_LISTING_CHANGED = 1,
  // A tab's path, history, flags, status text or selection moved. `id` = tab.
  BB_EVT_TAB_CHANGED = 2,
  // Panes or tabs were added, removed or activated.
  BB_EVT_LAYOUT_CHANGED = 3,
  // The settle predicate flipped. `a` = 1 when busy.
  BB_EVT_BUSY_CHANGED = 4,
  // The undo history gained, lost or moved an entry.
  BB_EVT_UNDO_CHANGED = 5,
  // An operation finished. `id` = operation, `a` = 1 when it succeeded.
  BB_EVT_OP_FINISHED = 6,
  // A setting changed. `text` = its key.
  BB_EVT_SETTINGS_CHANGED = 7,
  // A pane took the focus or changed its tab. `id` = pane.
  BB_EVT_PANE_CHANGED = 8,
} BbEventKind;

// The outcome of a call. Only `BB_OK` carries a result.
typedef enum BbStatus {
  // The call succeeded.
  BB_OK = 0,
  // The call failed; `bb_last_error` has the message.
  BB_ERR = 1,
  // Rust panicked inside the call; `bb_last_error` has the panic message.
  BB_PANIC = 2,
  // A handle or id named something that no longer exists.
  BB_STALE = 3,
  // Another operation holds the app; try again once it finishes.
  BB_BUSY = 4,
} BbStatus;

// Which seams the app is built over.
typedef enum BbFakes {
  // The real ones (for now: unattended defaults where the UI has not landed yet).
  BB_FAKES_NONE = 0,
  // The harness set: nothing is launched, elevation is declined, prompts are recorded.
  BB_FAKES_HARNESS = 1,
} BbFakes;

typedef enum BbOrientation {
  // Children sit side by side.
  BB_VERTICAL = 0,
  // Children are stacked.
  BB_HORIZONTAL = 1,
} BbOrientation;

// Which text of a tab to read.
typedef enum BbTabText {
  BB_TEXT_PATH = 0,
  BB_TEXT_TITLE = 1,
  BB_TEXT_STATUS = 2,
  // The error message; none (NULL) when the listing did not fail.
  BB_TEXT_ERROR = 3,
  // The message shown over an empty result; none when there is no such message.
  BB_TEXT_EMPTY = 4,
  // A note the listing is not saying for itself (a flat view stopped at its ceiling); none if none.
  BB_TEXT_NOTICE = 5,
  // The text of the active search box.
  BB_TEXT_SEARCH = 6,
  // The id of the column the list is sorted by.
  BB_TEXT_SORT_BY = 7,
} BbTabText;

typedef enum BbSelectOp {
  // Select exactly these rows.
  BB_SELECT_REPLACE = 0,
  BB_SELECT_ADD = 1,
  BB_SELECT_TOGGLE = 2,
  BB_SELECT_REMOVE = 3,
  BB_SELECT_ALL = 4,
  BB_SELECT_CLEAR = 5,
  BB_SELECT_INVERT = 6,
} BbSelectOp;

typedef enum BbDeleteMode {
  // Into the Windows Recycle Bin.
  BB_DELETE_RECYCLE = 0,
  // Into this app's holding folder; undoable for the session.
  BB_DELETE_STAGED = 1,
  // Erased in place; no undo.
  BB_DELETE_PERMANENT = 2,
} BbDeleteMode;

// Where an operation is.
typedef enum BbOpPhase {
  BB_OP_QUEUED = 0,
  BB_OP_RUNNING = 1,
  BB_OP_DONE = 2,
} BbOpPhase;

// What has the keyboard, as far as a shortcut needs to know.
typedef enum BbFocusKind {
  BB_FOCUS_FILE_LIST = 0,
  // A box somebody is typing in.
  BB_FOCUS_TEXT_EDITABLE = 1,
  // Selectable text nobody can type in.
  BB_FOCUS_TEXT_READ_ONLY = 2,
  BB_FOCUS_OTHER = 3,
} BbFocusKind;

// The app handle. One per process.
typedef struct BbApp BbApp;

// An immutable, reference-counted snapshot of a tab's rows in display order. Every view it hands out
// stays valid until the last reference is released.
typedef struct BbListing BbListing;

// A borrowed UTF-16 string view. Not NUL-terminated.
typedef struct BbStr {
  const uint16_t *ptr;
  uint32_t len;
} BbStr;

// A UTF-16 string the caller owns. Free it with `bb_str_free`. `ptr == NULL` means none.
typedef struct BbOwnedStr {
  uint16_t *ptr;
  uint32_t len;
} BbOwnedStr;

// An event delivered to the sink. Rust owns it; call `bb_event_free` when done. `text` is valid
// until then.
typedef struct BbEvent {
  enum BbEventKind kind;
  uint64_t id;
  uint64_t a;
  uint64_t b;
  struct BbStr text;
  const void *payload;
} BbEvent;

// How to build the app. A zero or empty field takes its default.
typedef struct BbAppConfig {
  // The data directory. Required.
  struct BbStr data_dir;
  enum BbFakes fakes;
  // How long typing in a search box waits before it runs. `UINT32_MAX` keeps the default (200);
  // the harness passes 0.
  uint32_t search_debounce_ms;
  // How long a folder change waits for quiet before the listing is merged. 0 keeps the default.
  uint32_t watch_debounce_ms;
  // Where deletes that cannot use the Recycle Bin are held; empty means the root of each volume.
  struct BbStr delete_staging_root;
  // Worker threads; 0 keeps the default (4).
  uint32_t workers;
} BbAppConfig;

// The receiver of events. Called on Rust threads: enqueue and return, do nothing else.
typedef void (*BbEventSink)(void *ctx, struct BbEvent *evt);

// A tab's scalars. Text lives behind `bb_tab_text`.
typedef struct BbTabState {
  uint64_t id;
  uint64_t pane;
  // Bumped on every new listing snapshot.
  uint64_t version;
  uint32_t count;
  uint32_t selected;
  bool can_back;
  bool can_forward;
  bool can_up;
  // The rows come from many folders (a search, or a flat view).
  bool is_flattened;
  // The flattening is a browse of a real folder, not a search result.
  bool is_flat_browse;
  bool is_flat_view;
  bool is_global_search;
  bool sort_descending;
  bool shows_content_matches;
  bool is_search_running;
  bool has_error;
  // 0 none, 1 preview, 2 metadata.
  uint32_t side_pane;
  double thumbnail_scale;
} BbTabState;

// One row, as views into the snapshot. Ask for visible rows at paint time only.
typedef struct BbRow {
  // Stable per path within a tab's browsing of one place, so hover survives a watcher merge.
  uint64_t row_id;
  struct BbStr name;
  struct BbStr full_path;
  // The folder a search or flat result came from, relative to where it was searched; empty
  // otherwise.
  struct BbStr relative_dir;
  // Bytes, or -1 for a folder whose size is not known (blank, never zero).
  int64_t size;
  // FILETIME ticks (100 ns since 1601 UTC); 0 when unknown.
  int64_t modified;
  int64_t created;
  int64_t accessed;
  // The Win32 `FILE_ATTRIBUTE_*` mask.
  uint32_t attributes;
  // `BB_ROW_*` bits.
  uint32_t flags;
} BbRow;

typedef struct BbOpInfo {
  enum BbOpPhase phase;
  // Meaningful once `phase` is `BB_OP_DONE`.
  bool ok;
} BbOpInfo;

typedef struct BbUndoState {
  bool can_undo;
  bool can_redo;
} BbUndoState;

// A key press, translated by the UI: `modifiers` is a mask (1 Ctrl, 2 Alt, 4 Shift) and `key` the
// key's name in this app's vocabulary (`P`, `F7`, `Del`, `Num+`). Rust decides what it means.
typedef struct BbKeyChord {
  uint32_t modifiers;
  struct BbStr key;
} BbKeyChord;

typedef struct BbFocusState {
  enum BbFocusKind focus;
  uint32_t flags;
} BbFocusState;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

// The core version, as a view into static storage.
struct BbStr bb_version(void);

// The message behind the last non-OK status on this thread. The view is valid until the next
// `bb_*` call on the same thread.
void bb_last_error(struct BbStr *out);

// Frees a string returned as `BbOwnedStr`. A null `ptr` is ignored.
void bb_str_free(struct BbOwnedStr s);

// Frees an event once the receiver is done with it.
void bb_event_free(struct BbEvent *evt);

// Always panics inside the guard, to prove `BB_PANIC` and that nothing unwinds across the boundary.
enum BbStatus bb_selftest_panic(void);

// Builds the app. Free it with `bb_app_destroy`.
enum BbStatus bb_app_create(const struct BbAppConfig *cfg, struct BbApp **out_app);

// Stops the app's threads and frees it. The handle is invalid afterwards. Events already queued by
// the sink stay valid until freed.
void bb_app_destroy(struct BbApp *app);

// Installs the event sink (or removes it with NULL). It is called on Rust threads and must only
// enqueue; `ctx` is passed back untouched.
void bb_app_set_event_sink(struct BbApp *app, BbEventSink sink, void *ctx);

// Opens the first tab at `start_path` and returns it.
enum BbStatus bb_app_initialize(struct BbApp *app, struct BbStr start_path, uint64_t *out_tab);

// The settle predicate: `(transferring && !paused) || loading || hydrating`.
bool bb_app_is_busy(struct BbApp *app);

// The Rust half of the harness's `state` verb, as JSON. Free with `bb_str_free`.
enum BbStatus bb_app_state_json(struct BbApp *app, struct BbOwnedStr *out_json);

// Splits `pane` (0 = the active pane), putting a new pane with one tab at `path` beside it. Returns
// the new pane.
enum BbStatus bb_pane_split(struct BbApp *app,
                            uint64_t pane,
                            enum BbOrientation orientation,
                            struct BbStr path,
                            uint64_t *out_pane);

enum BbStatus bb_pane_close(struct BbApp *app, uint64_t pane);

enum BbStatus bb_pane_activate(struct BbApp *app, uint64_t pane);

// Opens a tab in `pane` (0 = the active pane). Returns the new tab.
enum BbStatus bb_tab_open(struct BbApp *app,
                          uint64_t pane,
                          struct BbStr path,
                          bool activate,
                          uint64_t *out_tab);

enum BbStatus bb_tab_close(struct BbApp *app, uint64_t tab);

enum BbStatus bb_tab_activate(struct BbApp *app, uint64_t tab);

// Navigates to a folder. The listing arrives as `BB_EVT_LISTING_CHANGED`.
enum BbStatus bb_tab_navigate(struct BbApp *app, uint64_t tab, struct BbStr path);

enum BbStatus bb_tab_back(struct BbApp *app, uint64_t tab);

enum BbStatus bb_tab_forward(struct BbApp *app, uint64_t tab);

enum BbStatus bb_tab_up(struct BbApp *app, uint64_t tab);

enum BbStatus bb_tab_refresh(struct BbApp *app, uint64_t tab);

// Sets the folder search box (`global` false) or the whole-PC one (true). Empty text clears it.
enum BbStatus bb_tab_set_search(struct BbApp *app, uint64_t tab, struct BbStr text, bool global);

// Turns the flat branch view on or off.
enum BbStatus bb_tab_set_flat(struct BbApp *app, uint64_t tab, bool on);

// Sorts by a column id; the same column again flips the direction.
enum BbStatus bb_tab_set_sort(struct BbApp *app, uint64_t tab, struct BbStr column);

// The thumbnail slider, 0 (details list) to 1.
enum BbStatus bb_tab_set_thumbnail_scale(struct BbApp *app, uint64_t tab, double scale);

// 0 none, 1 preview, 2 metadata.
enum BbStatus bb_tab_set_side_pane(struct BbApp *app, uint64_t tab, uint32_t pane);

// Enter on a row: a folder opens in place, anything else goes to the launcher.
enum BbStatus bb_tab_open_item(struct BbApp *app, uint64_t tab, uint32_t index);

enum BbStatus bb_tab_state(struct BbApp *app, uint64_t tab, struct BbTabState *out_state);

enum BbStatus bb_tab_text(struct BbApp *app,
                          uint64_t tab,
                          enum BbTabText which,
                          struct BbOwnedStr *out_text);

// The breadcrumb as lines of `name<TAB>full path`, root first.
enum BbStatus bb_tab_breadcrumb(struct BbApp *app, uint64_t tab, struct BbOwnedStr *out_text);

// The columns the tab shows, as lines of
// `id<TAB>header<TAB>width<TAB>cell index<TAB>right aligned (0/1)<TAB>sortable (0/1)`. The cell
// index is what `bb_listing_cell` takes, or -1 for a column whose values this build cannot read.
enum BbStatus bb_tab_columns(struct BbApp *app, uint64_t tab, struct BbOwnedStr *out_text);

// The tab's current snapshot, with one reference for the caller (release it with
// `bb_listing_release`). NULL when the tab is gone.
const struct BbListing *bb_tab_listing(struct BbApp *app, uint64_t tab);

// Takes another reference to a snapshot.
void bb_listing_retain(const struct BbListing *l);

// Gives a reference back. The snapshot is freed with the last one.
void bb_listing_release(const struct BbListing *l);

uint32_t bb_listing_len(const struct BbListing *l);

uint64_t bb_listing_version(const struct BbListing *l);

enum BbStatus bb_listing_row(const struct BbListing *l, uint32_t index, struct BbRow *out_row);

// The formatted text of one cell. `column` is a cell index from `bb_tab_columns`. The view is valid
// as long as the snapshot is.
enum BbStatus bb_listing_cell(const struct BbListing *l,
                              uint32_t index,
                              uint32_t column,
                              struct BbStr *out_text);

// One gesture, one call: `idx` are positions in the snapshot the caller holds.
enum BbStatus bb_tab_select(struct BbApp *app,
                            uint64_t tab,
                            enum BbSelectOp op,
                            const uint32_t *idx,
                            uint32_t n);

// The selected rows' positions in the current snapshot, ascending. Writes up to `cap` of them and
// reports the total in `out_count`, so a caller can size a second call.
enum BbStatus bb_tab_selection(struct BbApp *app,
                               uint64_t tab,
                               uint32_t *out_idx,
                               uint32_t cap,
                               uint32_t *out_count);

// The selected rows' full paths, one per line.
enum BbStatus bb_tab_selection_paths(struct BbApp *app, uint64_t tab, struct BbOwnedStr *out_text);

// Moves the sources into a folder. Queued behind a running operation. The outcome arrives as
// `BB_EVT_OP_FINISHED`; a move is on the undo history.
enum BbStatus bb_op_move(struct BbApp *app,
                         const struct BbStr *src,
                         uint32_t n,
                         struct BbStr dest,
                         uint64_t *out_op);

enum BbStatus bb_op_copy(struct BbApp *app,
                         const struct BbStr *src,
                         uint32_t n,
                         struct BbStr dest,
                         uint64_t *out_op);

// Deletes the sources. `BB_BUSY` while another operation runs.
enum BbStatus bb_op_delete(struct BbApp *app,
                           const struct BbStr *src,
                           uint32_t n,
                           enum BbDeleteMode mode,
                           uint64_t *out_op);

// Renames the sources by a pattern ("Holiday" numbers across the selection, each keeping its
// extension). `BB_BUSY` while another operation runs.
enum BbStatus bb_op_rename(struct BbApp *app,
                           const struct BbStr *src,
                           uint32_t n,
                           struct BbStr pattern,
                           uint64_t *out_op);

// Pauses or resumes the running transfer, mid-file. The queue stays held while paused.
enum BbStatus bb_op_pause(struct BbApp *app, bool paused);

// Cancels a queued operation.
enum BbStatus bb_op_cancel(struct BbApp *app, uint64_t op);

// An operation's phase, and (in `out_message`, when non-NULL) the sentence it ended with.
enum BbStatus bb_op_state(struct BbApp *app,
                          uint64_t op,
                          struct BbOpInfo *out_info,
                          struct BbOwnedStr *out_message);

// Undoes `steps` entries of the history, or redoes `-steps` when negative. `BB_BUSY` while another
// operation runs.
enum BbStatus bb_undo_step(struct BbApp *app, int32_t steps, uint64_t *out_op);

enum BbStatus bb_undo_state(struct BbApp *app, struct BbUndoState *out_state);

// Which command, if any, owns this key press in this state. `out_command` is NULL when it is nobody's
// and must carry on to the focused control. Says nothing about whether the command is available: a
// claimed chord is the command's even when it can do nothing.
enum BbStatus bb_key_dispatch(struct BbApp *app,
                              struct BbKeyChord chord,
                              struct BbFocusState focus,
                              struct BbOwnedStr *out_command);

// Runs a command against the active tab. Fails with the reason `bb_command_why_not` gives.
enum BbStatus bb_command_run(struct BbApp *app, struct BbStr id);

// Why a command cannot run now, in words fit to show beside it; `out_reason` is NULL when it can.
// `BB_ERR` for an id nobody knows. This is the one gate a menu row, a key and the palette share.
enum BbStatus bb_command_why_not(struct BbApp *app, struct BbStr id, struct BbOwnedStr *out_reason);

// One setting as JSON text (`null` for an unset optional). Keys are the settings file's.
enum BbStatus bb_settings_get(struct BbApp *app, struct BbStr key, struct BbOwnedStr *out_json);

// Sets one setting from JSON text, validating it against the model and writing the file at once.
enum BbStatus bb_settings_set(struct BbApp *app, struct BbStr key, struct BbStr json_value);

// How many colour tokens a palette holds.
uint32_t bb_theme_token_count(void);

// The current theme's colours as 0xAARRGGBB, one per token in token order. Writes up to `cap` and
// reports how many exist in `out_count`.
enum BbStatus bb_theme_palette(struct BbApp *app,
                               uint32_t *out_argb,
                               uint32_t cap,
                               uint32_t *out_count);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* BBCORE_H */
