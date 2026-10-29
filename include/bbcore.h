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

// Bits 4..6 are what a folder comparison says about the row (`compare_view::state_number`: 0 nothing,
// 1 not compared, 2 same, 3 only here, 4 newer, 5 older, 6 differs): `(flags >> 4) & 7`.
#define BB_ROW_COMPARE_SHIFT 4

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
  // A service's state moved; re-read it. `a` = the service: 1 settings page, 2 theme (re-push the
  // palette), 3 menu bar/toolbars, 4 keymap, 5 palette, 6 Keyboard page, 7 Menus and toolbars
  // page, 8 saved searches/workspaces/bookmarks, 9 index banner/status, 10 columns, 11 compare,
  // 12 command availability.
  BB_EVT_MODEL_CHANGED = 9,
  // A tab shows shell-property columns with cells the host has not read. `id` = tab. Ask with
  // `bb_props_requests`, answer with `bb_props_supply`.
  BB_EVT_PROPERTIES_NEEDED = 10,
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

// What one launch is: its request, whether another copy took it, and the instance claim.
typedef struct BbLaunch BbLaunch;

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

// As [`BbAppConfig`], with the settings the harness and the tests need beyond it.
typedef struct BbAppConfigEx {
  struct BbAppConfig base;
  // How long the Settings page waits for quiet before writing the file. `UINT32_MAX` keeps the
  // default (400); 0 writes at once.
  uint32_t settings_debounce_ms;
  // Put the real index client behind a launcher that declines the elevation prompt (the
  // harness's `--index-declined`).
  bool index_declined;
} BbAppConfigEx;

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
  // The preview pane's mode (0 auto, 1 raw, 2 hex) and its two per-tab switches (wrap, 100%).
  uint32_t preview_mode;
  bool preview_wrap;
  bool preview_fit;
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

// The host's side of the property system. Every callback returns `BB_OK` with `*out` set to a JSON
// view that stays valid until the host's next call on that thread, or a failure status.
//
// - `details(user, path, out)`: `[{canonical, name, value}]` for every property with a value.
// - `read_fields(user, path, canonicals_json, ask_writable, out)`: `null` when no store would open,
//   else `{writable: [canonical]|null, values: [{canonical, text?, number?, utc?}]}`; `utc` is a
//   FILETIME as a 64-bit integer. `writable` is only filled when `ask_writable` (it costs an
//   exclusive write-open).
// - `write_fields(user, path, values_json, out_error)`: sets `[{canonical, text?|number?|utc?}]`
//   and commits in place; a failure puts the handler's words in `*out_error`.
typedef struct BbPropertyHost {
  void *user;
  enum BbStatus (*details)(void *user, struct BbStr path, struct BbStr *out);
  enum BbStatus (*read_fields)(void *user,
                               struct BbStr path,
                               struct BbStr canonicals_json,
                               bool ask_writable,
                               struct BbStr *out);
  enum BbStatus (*write_fields)(void *user,
                                struct BbStr path,
                                struct BbStr values_json,
                                struct BbStr *out_error);
} BbPropertyHost;

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

// As `bb_app_create`, taking the extended configuration.
enum BbStatus bb_app_create_ex(const struct BbAppConfigEx *cfg, struct BbApp **out_app);

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

// Lowers how many rows a flat listing shows before it stops and says so (the harness's `flat-cap`);
// takes effect on the next flat listing. The default is the real ceiling, 50,000.
enum BbStatus bb_flat_set_cap(struct BbApp *app, uint32_t cap);

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

// Cancels an operation: one still waiting never starts, the running one is told to stop (and a
// pause is lifted so it can notice).
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

// Sets one setting from JSON text, validating it against the model. It takes effect at once; a
// setting the Settings page owns is written on a 400 ms debounce (`bb_settings_flush` writes it
// now), one a service owns (keymap, bars, theme) is written at once.
enum BbStatus bb_settings_set(struct BbApp *app, struct BbStr key, struct BbStr json_value);

// How many colour tokens a palette holds.
uint32_t bb_theme_token_count(void);

// The current theme's colours as 0xAARRGGBB, one per token in token order. Writes up to `cap` and
// reports how many exist in `out_count`.
enum BbStatus bb_theme_palette(struct BbApp *app,
                               uint32_t *out_argb,
                               uint32_t cap,
                               uint32_t *out_count);

// The name the New dialog opens with: the type's default, stepped aside to "(2)" if taken. `kind`
// is 0 for a folder, 1 for a file; `extension` is the file type's, with its dot.
enum BbStatus bb_new_item_suggest(struct BbApp *app,
                                  uint64_t tab,
                                  struct BbStr directory,
                                  uint32_t kind,
                                  struct BbStr extension,
                                  struct BbOwnedStr *out_name);

// What creating `name` would produce, without changing anything, as JSON
// `{directory, name, target, ok, rejected}`.
enum BbStatus bb_new_item_plan(struct BbApp *app,
                               uint64_t tab,
                               struct BbStr directory,
                               uint32_t kind,
                               struct BbStr name,
                               struct BbOwnedStr *out_json);

// Creates a folder (`kind` 0) or file (1) in `directory` (empty: the folder the tab shows) and
// selects it when its listing arrives. Returns the operation.
enum BbStatus bb_new_item(struct BbApp *app,
                          uint64_t tab,
                          struct BbStr directory,
                          uint32_t kind,
                          struct BbStr name,
                          uint64_t *out_op);

// The file types the New submenu offers, as JSON `[{label, extension, templatePath}]`.
enum BbStatus bb_new_file_types(struct BbApp *app, struct BbOwnedStr *out_json);

// What closing the window would save, as the settings file spells it.
enum BbStatus bb_session_capture(struct BbApp *app, struct BbOwnedStr *out_json);

// Writes the pane and tab arrangement into the settings now.
enum BbStatus bb_session_save(struct BbApp *app);

// The launch rule: the saved session when asked for and usable, otherwise one tab at the startup
// folder (or `fallback`). Returns the active tab.
enum BbStatus bb_session_restore(struct BbApp *app, struct BbStr fallback, uint64_t *out_tab);

// Captures, prunes and rebuilds the arrangement in place (the harness's `session`). `out_rebuilt`
// is false when nothing in it was worth keeping and the arrangement was left alone.
enum BbStatus bb_session_roundtrip(struct BbApp *app, bool *out_rebuilt);

// Moves a tab to `index` within its pane (clamped).
enum BbStatus bb_tab_move(struct BbApp *app, uint64_t tab, uint32_t index);

// Moves a tab into another pane, activating it there; a pane it empties closes.
enum BbStatus bb_tab_move_to_pane(struct BbApp *app, uint64_t tab, uint64_t pane);

// Exchanges the active pane's place with the next one's.
enum BbStatus bb_pane_swap(struct BbApp *app);

// Gives every pane the same share of the space.
enum BbStatus bb_pane_equalise(struct BbApp *app);

// Every command in the state a menu would draw it, as JSON
// `[{id, name, category, icon, gesture, context, owner, whyNot, checked, caption}]`. `whyNot` is
// **the one gate**: the registry's answer plus "a dialog is up" and "Settings is open".
enum BbStatus bb_command_list(struct BbApp *app, struct BbOwnedStr *out_json);

// One command's state, as an element of `bb_command_list`. `BB_ERR` for an id nobody knows.
enum BbStatus bb_command_state(struct BbApp *app, struct BbStr id, struct BbOwnedStr *out_json);

// Runs a command through the gate. `out_needs_ui` is true when the command is the host's to carry
// out (a dialog, the clipboard, focus): it was available, and nothing was done.
enum BbStatus bb_command_execute(struct BbApp *app, struct BbStr id, bool *out_needs_ui);

// A key press the way the window's one handler takes it: whose it is given what has focus, closing
// the palette for anything but its own chord, then through the gate. JSON
// `{claimed, command?, result?: "ran"|"ui"|"unavailable"|"ignored", reason?}`. A claimed chord is
// the command's even when it cannot run; the host marks the key handled. `repeat` is a held key.
enum BbStatus bb_key_press(struct BbApp *app,
                           struct BbKeyChord chord,
                           struct BbFocusState focus,
                           bool repeat,
                           struct BbOwnedStr *out_json);

// Gives a command exactly these shortcuts (texts like `Ctrl+Shift+K`; none leaves it with no key),
// taking each from whoever had it. Written at once.
enum BbStatus bb_keymap_set_chords(struct BbApp *app,
                                   struct BbStr id,
                                   const struct BbStr *chords,
                                   uint32_t count);

// Puts one command back to the shortcuts it ships with (`id` empty: all of them).
enum BbStatus bb_keymap_reset(struct BbApp *app, struct BbStr id);

// The shortcut a menu prints beside a command (empty when it has none), for text that names a key.
enum BbStatus bb_keymap_gesture(struct BbApp *app, struct BbStr id, struct BbOwnedStr *out_text);

// The Keyboard page: `{filter, selected, recording, finding, capturing, canReassign, text,
// hasChanged, rows:[{heading, name, id, category, chords, changed, ...}]}`.
enum BbStatus bb_keys_page(struct BbApp *app, struct BbOwnedStr *out_json);

// One verb of the Keyboard page: `filter <text>`, `select <id>`, `change`, `add`, `find`, `cancel`,
// `press <chord>`, `reassign`, `remove`, `reset`, `reset-all`.
enum BbStatus bb_keys_do(struct BbApp *app, struct BbStr verb, struct BbStr arg);

// Opens the palette on `seed` (empty: the empty state).
enum BbStatus bb_palette_open(struct BbApp *app, struct BbStr seed);

// `{open, query, hint, empty, selected, rows:[{index, kind: heading|entry|category, text, id, icon,
// gesture, detail, available, pinned, canPin, canBind, token, count, highlights, selected}]}`.
enum BbStatus bb_palette_state(struct BbApp *app, struct BbOwnedStr *out_json);

// One verb: `type <text>`, `down [n]`, `up [n]`, `home`, `end`, `select <row>`, `pick` (Enter),
// `complete` (Tab), `pin`, `shortcut`, `close`. The result is JSON; `pick` answers
// `{ran, ui?, command?, narrowed?, unavailable?}`.
enum BbStatus bb_palette_do(struct BbApp *app,
                            struct BbStr verb,
                            struct BbStr arg,
                            struct BbOwnedStr *out_json);

// Reads the command bars. `what`: `state` (the arrangement in force), `menubar` (the headings),
// `menu` (arg: a heading's name or position, as it would open now), `toolbar` (arg: `title` or
// `row1`..., as it shows now), `list` (arg: a list id; the rows it expands to), `page` (the
// Menus and toolbars page's editor).
enum BbStatus bb_bars_query(struct BbApp *app,
                            struct BbStr what,
                            struct BbStr arg,
                            struct BbOwnedStr *out_json);

// One verb of the command-bar service: `reset`, `save <name>`, `apply <name>`,
// `rename <old> to <new>`, `remove <name>`, `set-json <arrangement>`, `menu <caption> <tokens>`
// (add or replace a top-level menu; tokens are command ids, `-` and `@list`).
enum BbStatus bb_bars_do(struct BbApp *app, struct BbStr verb, struct BbStr arg);

// Replaces one toolbar's items. `target`: `title` or `row1`...; `tokens`: command ids, `-` for a
// separator, `@list` for a list; `show_text`: 1 buttons carry their names, 0 they do not, -1 keep.
enum BbStatus bb_bars_set_toolbar(struct BbApp *app,
                                  struct BbStr target,
                                  struct BbStr tokens,
                                  int32_t show_text);

// One verb of the Menus and toolbars page's editor: `select <bar>`, `pick <row>`, `open <row>`,
// `up`, `add <id|name>`, `add-separator`, `add-menu`, `remove [row]`, `nudge [row] up|down`,
// `move <from> <to>`, `name <caption>`, `show-text on|off`, `row-show-text on|off`, `add-row`,
// `remove-row`, `filter <text>`, `unplaced on|off`, `reset`, `apply-saved <name>`,
// `remove-saved <name>`, `select-saved <name>`, `save <name>`, `rename-saved <old> to <new>`.
enum BbStatus bb_barpage_do(struct BbApp *app, struct BbStr verb, struct BbStr arg);

// The columns a tab shows: `{customized, columns:[{id, header, width, cell, rightAligned,
// sortable, injected, property, canonical}]}`. `cell` is what `bb_listing_cell` takes.
enum BbStatus bb_columns_state(struct BbApp *app, uint64_t tab, struct BbOwnedStr *out_json);

// One verb on a tab's columns: `add <id>`, `remove <id>`, `move <id> <index>`, `width <id> <dips>`,
// `reset`, `default` (make this the layout new tabs start from), `picked <id>,<id>...`,
// `order <id>,<id>...` (after a header drag).
enum BbStatus bb_columns_do(struct BbApp *app, uint64_t tab, struct BbStr verb, struct BbStr arg);

// What the Add column popup offers for `search`: `{loading, full, groups:[{title, items:[{id,
// header, detail}]}]}`.
enum BbStatus bb_columns_candidates(struct BbApp *app,
                                    uint64_t tab,
                                    struct BbStr search,
                                    struct BbOwnedStr *out_json);

// The properties this machine reported, as JSON `[[canonical, display], ...]`.
enum BbStatus bb_columns_set_machine_properties(struct BbApp *app, struct BbStr json);

// The shell-property cells the host should read for rows `from..from+count` (count 0: the first
// 256), as JSON `[[path, canonical], ...]`. Each cell is handed out once. Until the host has asked
// (and while it owes answers) the tab counts as hydrating. Only meaningful after
// `bb_ui_set_flag("propertyProvider", true)`.
enum BbStatus bb_props_requests(struct BbApp *app,
                                uint64_t tab,
                                uint32_t from,
                                uint32_t count,
                                struct BbOwnedStr *out_json);

// The host's answers, as JSON `[{path, canonical, value?: {display, number?, date?}}]`: `date` is
// UTC FILETIME ticks, and a missing `value` means the handler had none (the cell stays blank and
// is not asked again).
enum BbStatus bb_props_supply(struct BbApp *app, uint64_t tab, struct BbStr json);

// The thumbnail view's state: `{scale, isThumbnails, lastScale, aspect:{width, height, ratio,
// text}}`. The thumbnails themselves are the host's.
enum BbStatus bb_tab_tile_state(struct BbApp *app, uint64_t tab, struct BbOwnedStr *out_json);

// `{searches:[{name, query, scope: current|folder|pc, scopePath, scopeText}], workspaces:[{name,
// shape, folders}], bookmarks:[{path, name, isDirectory, hidden}], workspacesPlacement,
// savedSearchesPlacement}`.
enum BbStatus bb_saved_state(struct BbApp *app, struct BbOwnedStr *out_json);

// One verb. On `tab` (0: none): `save-search <name> [current|folder|pc]` saves what the tab is
// searching for. Otherwise: `run-search <name>`, `run-search-new-tab <name>`,
// `rename-search <old> to <new>`, `remove-search <name>`, `save-workspace <name>`,
// `switch-workspace <name>`, `rename-workspace <old> to <new>`, `remove-workspace <name>`,
// `toggle-bookmark <path>`, `open-bookmark <here|new-tab|pane-right|pane-below> <path>`.
enum BbStatus bb_saved_do(struct BbApp *app, uint64_t tab, struct BbStr verb, struct BbStr arg);

// Select (`select` true) or deselect by pattern on `tab`, as JSON `{total, matched, problem,
// summary, usable}`: what `pattern` picks out of the rows on screen, said as it is typed. With
// `apply` the matches are added to the selection or taken out of it.
enum BbStatus bb_select_pattern(struct BbApp *app,
                                uint64_t tab,
                                struct BbStr pattern,
                                bool select,
                                bool apply,
                                struct BbOwnedStr *out_json);

// What a Save workspace (`kind` = `workspace`) or Save menus-and-toolbars (`command-bars`) dialog
// shows for `name`, as JSON `{default, problem, taken}`: the name it opens with, why this one
// cannot be saved (null when it can), and whether saving replaces one of that name.
enum BbStatus bb_save_name(struct BbApp *app,
                           struct BbStr kind,
                           struct BbStr name,
                           struct BbOwnedStr *out_json);

// Whether `path` is bookmarked.
bool bb_is_bookmarked(struct BbApp *app, struct BbStr path);

// Puts `text` on the active tab's status bar: an outcome only the host knows (a copied path).
enum BbStatus bb_status_set(struct BbApp *app, struct BbStr text);

// The name a new saved search (or, with `workspace`, a new workspace) starts with.
enum BbStatus bb_saved_default_name(struct BbApp *app,
                                    uint64_t tab,
                                    bool workspace,
                                    struct BbOwnedStr *out_name);

// Writes what the Settings page still holds. Called on Back and on window close.
enum BbStatus bb_settings_flush(struct BbApp *app);

// `{open, page, pages:[{id, name, icon}], problem}`.
enum BbStatus bb_settings_page_state(struct BbApp *app, struct BbOwnedStr *out_json);

// Opens the Settings page on `page` (a page id; empty: General). While it is up only app-wide
// commands run.
enum BbStatus bb_settings_page_open(struct BbApp *app, struct BbStr page);

// Back: flushes, and refuses with `BB_ERR` (the reason in `bb_last_error`) while a list the page
// holds is still unwritable.
enum BbStatus bb_settings_page_close(struct BbApp *app);

// `{id, name, isDark, following, lightSlot, darkSlot, appearance:{dark, highContrast},
// dimmedIconOpacity, available:[{id, name, isDark, builtIn, current}], overridden, dirty, issues}`.
enum BbStatus bb_theme_state(struct BbApp *app, struct BbOwnedStr *out_json);

// Finds the files under `path` that are byte-for-byte the same (the folder is crawled first, so no index
// helper is needed). `min_size_bytes` floors the files considered. JSON `{availability, incomplete,
// wastedBytes, groups:[{sizeBytes, hash, keep:{newest, oldest, shallowest}, files:[{path, name, dir,
// sizeBytes, modified, hidden, hardlinks}]}]}`; `keep` is the index of the copy each strategy keeps.
// Blocks: call it from a worker.
enum BbStatus bb_duplicates_scan(struct BbApp *app,
                                 struct BbStr path,
                                 int64_t min_size_bytes,
                                 struct BbOwnedStr *out_json);

// The search syntax help, grouped: `[{title, blurb, entries:[{example, meaning}]}]`. The same table
// the parser reads, so the help cannot advertise a key the parser does not implement.
enum BbStatus bb_search_syntax_help(struct BbOwnedStr *out_json);

// Every colour token, for the theme editor: `[{key, group, name, description, core, argb,
// overridden}]` in editor display order.
enum BbStatus bb_theme_tokens(struct BbApp *app, struct BbOwnedStr *out_json);

// The host tells us what Windows looks like now. Following Windows swaps the theme (and raises
// `BB_EVT_MODEL_CHANGED` 2: re-push the palette); high contrast wins over both slots.
enum BbStatus bb_theme_set_system_appearance(struct BbApp *app, bool is_dark, bool high_contrast);

// One verb: `select <id>`, `follow on|off`, `slot light|dark <id>`, `override <token> <#hex>`,
// `clear-override <token>`, `persist`, `revert`, `reset-all`, `save-as <name>`, `import <path>`,
// `delete <id>`. `save-as` and `import` answer the new theme's id in `out_result`.
enum BbStatus bb_theme_do(struct BbApp *app,
                          struct BbStr verb,
                          struct BbStr arg,
                          struct BbOwnedStr *out_result);

// `{status, canRetry, canStart, presence, anyIndexed, building, banner:{show, message, canStart}}`.
enum BbStatus bb_index_state(struct BbApp *app, struct BbOwnedStr *out_json);

// `dismiss-banner` ("not now": not persisted), `start` (the one thing that raises the elevation
// prompt), `retry`.
enum BbStatus bb_index_do(struct BbApp *app, struct BbStr verb);

// Tells us a fact about the window that decides what can run: `clipboardHasFiles`,
// `previewHasMedia`, `propertyProvider` (the host reads shell properties for property columns).
enum BbStatus bb_ui_set_flag(struct BbApp *app, struct BbStr name, bool value);

// A modal dialog opened (`true`) or closed. While one is up nothing about the folders can run.
enum BbStatus bb_ui_set_dialog_open(struct BbApp *app, bool open);

// Starts or stops comparing the active pane with the next. (Only the switch and its gates live
// here; the comparison engine is not part of this layer.)
enum BbStatus bb_compare_toggle(struct BbApp *app);

// A model of the operations UI as JSON text: `queue`, `history`, `conflicts`, `elevation`,
// `right-drop-menu`, `drop-allowed`, `drop-decide`, `dragout-decide`, `rename-preview`,
// `context-menu`, `menu-arrange`, `menu-tidy`, `compress-defaults`, `archive-of`.
enum BbStatus bb_ops_query(struct BbApp *app,
                           struct BbStr what,
                           struct BbStr arg,
                           struct BbOwnedStr *out_json);

// A verb of the operations UI. `out_json` (when non-NULL) receives `{"op": N}` for a verb that
// started work, `{}` otherwise.
enum BbStatus bb_ops_do(struct BbApp *app,
                        struct BbStr verb,
                        struct BbStr arg,
                        struct BbOwnedStr *out_json);

// What the preview pane shows for the tab's selection, as JSON `{state, title, subtitle, message,
// canChooseMode, path, lines, spans, footer, entries, ...}`; see `bb-app/src/preview.rs`. `mode` is
// 0 auto, 1 raw text, 2 hex, 3 the document fallback (the shell declined; read the bytes as text).
// Blocking: call it from a worker thread.
enum BbStatus bb_preview_build(struct BbApp *app,
                               uint64_t tab,
                               uint32_t mode,
                               struct BbOwnedStr *out_json);

// As `bb_preview_build` for one path that need not be selected anywhere.
enum BbStatus bb_preview_build_path(struct BbApp *app,
                                    struct BbStr path,
                                    uint32_t mode,
                                    struct BbOwnedStr *out_json);

// Starts playing the GIF at `path` and returns once its first frame is ready; the rest decode on
// a thread of their own. `out_json` is `{animated, handle, width, height, frames, durationMs}`, or
// `{animated: false, reason: "still"|"tooLarge"|"unreadable", frames}` with `handle` 0 when it
// should be shown as a picture. Blocking: call it from a worker thread.
enum BbStatus bb_gif_open(struct BbApp *app,
                          struct BbStr path,
                          uint64_t *out_handle,
                          struct BbOwnedStr *out_json);

// How many frames have been decoded, and `flags`: bit 0 all of them, bit 1 decoding ended early
// (nothing more is coming).
enum BbStatus bb_gif_ready(struct BbApp *app,
                           uint64_t handle,
                           uint32_t *out_ready,
                           uint32_t *out_flags);

// Copies frame `index` (BGRA, `width * height * 4` bytes) into `dst`, or the last one decoded if it
// has not arrived yet; `out_index` says which it really was.
enum BbStatus bb_gif_frame(struct BbApp *app,
                           uint64_t handle,
                           uint32_t index,
                           uint8_t *dst,
                           size_t capacity,
                           uint32_t *out_index);

// The loop arithmetic: the frame on screen at `position_ms` (any position; it wraps), how long
// until the frame changes (at least 1), and where `frame` starts.
enum BbStatus bb_gif_position(struct BbApp *app,
                              uint64_t handle,
                              int64_t position_ms,
                              uint32_t *out_frame,
                              int64_t *out_until_next);

enum BbStatus bb_gif_start_of(struct BbApp *app, uint64_t handle, uint32_t frame, int64_t *out_ms);

// Stops the decoding and lets the frames go.
enum BbStatus bb_gif_close(struct BbApp *app, uint64_t handle);

// Installs the host's window onto the Windows property system (all callbacks null removes it).
enum BbStatus bb_set_property_host(struct BbApp *app, struct BbPropertyHost host);

// Points the tab's metadata pane at its selection and reads it (blocking, but only as long as the
// files take). Nothing happens when neither the selected paths nor their modified times moved,
// unless `force`: a refresh that changes neither must not wipe what is half typed.
enum BbStatus bb_metadata_load(struct BbApp *app, uint64_t tab, bool force);

// The pane as JSON: `{title, summary, isBusy, isEmpty, files, fields, details, canHoldPicture,
// hasPicture, pictureNote, located, locationNote, canApply, canRemove, canRemoveLocation,
// canRemoveAll, canRemovePicture}`; see `bb-app/src/metadata_pane.rs`.
enum BbStatus bb_metadata_model(struct BbApp *app, uint64_t tab, struct BbOwnedStr *out_json);

// Types into a box. Only a change of the text counts as touching it.
enum BbStatus bb_metadata_set_text(struct BbApp *app,
                                   uint64_t tab,
                                   struct BbStr id,
                                   struct BbStr text);

// Puts a box back to what the files hold.
enum BbStatus bb_metadata_revert(struct BbApp *app, uint64_t tab, struct BbStr id);

// A day chosen in a date box's calendar; the time already in the box is kept.
enum BbStatus bb_metadata_pick_date(struct BbApp *app,
                                    uint64_t tab,
                                    struct BbStr id,
                                    int32_t year,
                                    uint32_t month,
                                    uint32_t day);

// Presses one of the pane's buttons: 0 Apply, 1 remove the location, 2 remove everything, 3 remove
// the cover picture, 4 set the cover picture to the `len` bytes at `data`. Queued like any write
// and recorded in the undo history; `BB_ERR` says why the button would not have worked. The pane
// updates itself when the operation finishes and `BB_EVT_MODEL_CHANGED` 13 arrives.
enum BbStatus bb_metadata_act(struct BbApp *app,
                              uint64_t tab,
                              uint32_t action,
                              const uint8_t *data,
                              size_t len,
                              uint64_t *out_op);

// The cover the selection shares, as the image file it is. Call with `dst` null to learn the
// length; `*out_len` is 0 when there is none.
enum BbStatus bb_metadata_picture(struct BbApp *app,
                                  uint64_t tab,
                                  uint8_t *dst,
                                  size_t capacity,
                                  size_t *out_len);

// Reads both sides of each path and settles its verdict to "same" where the bytes are identical;
// nothing else moves. `out_identical` is how many were. Blocking: call it from a worker.
enum BbStatus bb_compare_settle(struct BbApp *app,
                                const struct BbStr *paths,
                                uint32_t n,
                                uint32_t *out_identical);

// Renames by the expanded panel's rule, given as the settings file spells it:
// `{template, find, replace, useRegex, matchCase, scope, case, counterStart, counterStep}` (a
// missing key keeps its default). A template that names a tag has each file's metadata read first.
enum BbStatus bb_op_rename_rule(struct BbApp *app,
                                const struct BbStr *src,
                                uint32_t n,
                                struct BbStr rule_json,
                                uint64_t *out_op);

// Commits what the undo history is holding and empties it (Settings, History, Clear).
enum BbStatus bb_undo_clear(struct BbApp *app);

// Writes a set of small real files into `dir` (created if need be): `metadata` (a camera picture,
// a bare picture, two songs), `media` (the same tone as FLAC, WAV, M4A and WMA, and a two-track
// MP4), `formats` (PNG, WebP, TIFF, a Word document, an EPUB, AIFF and Opus), `windows` (an `.asf`
// and a GIF no codec writes). None of them is written by the code under test: Windows' own
// encoders, or bytes laid out by hand from the specifications.
enum BbStatus bb_fixture_write(struct BbStr kind, struct BbStr dir);

// The scale after `notches` wheel notches, relative to "fit" (1). Pure arithmetic: the view owns
// the transform, the core owns the rules.
double bb_zoom_step(double scale, double notches, double min_scale);

double bb_zoom_clamp(double scale, double min_scale);

// The offset that keeps the image point under the cursor where it is when the scale changes.
double bb_zoom_offset(double offset, double anchor, double old_scale, double new_scale);

// Keeps the picture from being dragged out of view (centred where it is smaller than the pane).
double bb_zoom_clamp_offset(double offset,
                            double origin,
                            double length,
                            double scale,
                            double viewport);

// The row to select to go on to the next video after the selected one (`-1`: there is none).
enum BbStatus bb_preview_next_media(struct BbApp *app, uint64_t tab, int64_t *out_row);

// What the file holds now, read from disk through the codec that writes it: one field by id, or
// `location` / `picture` as "yes" or an empty string. `BB_ERR` says why it could not be read.
enum BbStatus bb_metadata_peek(struct BbApp *app,
                               struct BbStr path,
                               struct BbStr what,
                               struct BbOwnedStr *out_text);

// Runs the installer's hooks (`--veloapp-*`) and applies an update that is waiting. Call it first
// in `WinMain`. Returns normally for an ordinary launch; for a hook the process exits inside it.
// `argv` is the whole command line with the program name first, as `CommandLineToArgvW` gives it.
enum BbStatus bb_startup(int32_t argc, const uint16_t *const *argv);

// Parses the command line and claims the per-user instance. If another copy is running and takes
// the request, `bb_launch_handed_off` is true and the caller should free this and exit. Otherwise
// (this is the first copy, or the first could not be reached) carry on starting normally.
enum BbStatus bb_launch_claim(int32_t argc,
                              const uint16_t *const *argv,
                              struct BbStr data_dir,
                              struct BbLaunch **out_launch);

// True when a running copy took this launch's request: free the launch and exit.
bool bb_launch_handed_off(const struct BbLaunch *launch);

// The folder the command line named for the window to open on, or empty. A view valid until
// `bb_launch_free`.
struct BbStr bb_launch_start_path(const struct BbLaunch *launch);

// True when `BERTBROWSER_STARTUP_TRACE` is set: this launch is a measurement. The window is to be
// parked offscreen without activation or a taskbar button.
bool bb_trace_enabled(void);

// Records a mark on the way up (`windowBuilt`, `windowShown`, `contentRendered`, ...) when tracing.
void bb_trace_mark(struct BbStr name);

// Carries out a command that starts another program for the user and that `bb_command_execute`
// handed back as the host's (`file.run-as-admin`, `file.open-terminal`, `file.open-vscode`, a
// `custom:` command): the program, its arguments and its working directory are decided in Rust
// from the selection, and started through the one launcher. The outcome is on the status line.
enum BbStatus bb_launch_command(struct BbApp *app, struct BbStr id);

// As `bb_launch_command`, on `folder` rather than the selection: the folder tree's menu.
enum BbStatus bb_launch_command_at(struct BbApp *app, struct BbStr id, struct BbStr folder);

// Who opens folders and drives on this machine, as JSON: `{"state": "NotRegistered" |
// "RegisteredToThisApp" | "RegisteredToThisAppStale" | "RegisteredToAnotherApp", "other": <program
// or null>, "executable": <this app's path>}`. Reads the registry; free with `bb_str_free`. Product
// only: the harness never calls it, so a script cannot read the machine's registry.
enum BbStatus bb_folder_handler_state(struct BbOwnedStr *out_json);

// Registers this app as the shell's handler for folders and drives (`open_here`), or removes
// exactly what it registered. `BB_ERR` when the registry refused, in which case nothing is left
// half-written. Writes `HKEY_CURRENT_USER` only. Product only, and only ever on the user's say-so.
enum BbStatus bb_folder_handler_set(bool open_here);

// Frees the launch and releases the per-user instance claim. Call it last, after the app.
void bb_launch_free(struct BbLaunch *launch);

// Once the main window exists: answer other launches (raising `hwnd`, unless the foreground is
// full screen), open the rest of the command line, and start the background work. `app` must
// outlive the launch. A benchmark launch (`bb_trace_enabled`) stops after the listener: everything
// that acts on the machine is left out.
enum BbStatus bb_launch_serve(struct BbLaunch *launch, struct BbApp *app, void *hwnd);

// Runs one tool verb (`checksum.hash`, `filecompare.open`, `diskusage.model`, `changes.open`,
// `compare.sync`, ...) with its JSON argument (empty for none) and answers JSON. Blocks for as long
// as the files take: call it from a worker. A verb that starts an operation answers `{op}` and the
// operation finishes with `BB_EVT_OP_FINISHED`; the vocabulary is `bb-app/src/tools.rs`.
enum BbStatus bb_tools(struct BbApp *app,
                       struct BbStr verb,
                       struct BbStr args_json,
                       struct BbOwnedStr *out_json);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* BBCORE_H */
