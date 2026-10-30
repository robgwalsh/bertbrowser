//! The exports for the services around the tabs: new items, the session, commands and keys, the
//! palette, the menu bar and toolbars, columns and their property seam, saved searches and
//! workspaces, the Settings page, themes, the search index and the host's window state.
//!
//! Models (a page, a list) cross as JSON text and arrive with `BB_EVT_MODEL_CHANGED`; the `*_do`
//! exports take a verb and its argument so a page's whole vocabulary is one function. Strings are
//! UTF-16 as everywhere; results are freed with `bb_str_free`.

use super::*;

use bb_core::newitem::NewItemKind;

fn text_of(s: BbStr) -> String {
    unsafe { s.to_string_lossy() }
}

unsafe fn put(out_json: *mut BbOwnedStr, text: &str) -> Result<(), Fail> {
    *unsafe { out(out_json, "out_json")? } = BbOwnedStr::from_str(text);
    Ok(())
}

fn kind_of(kind: u32) -> NewItemKind {
    if kind == 0 {
        NewItemKind::Folder
    } else {
        NewItemKind::File
    }
}

// ---- new items ----

/// The name the New dialog opens with: the type's default, stepped aside to "(2)" if taken. `kind`
/// is 0 for a folder, 1 for a file; `extension` is the file type's, with its dot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_new_item_suggest(
    app: *mut BbApp,
    tab: u64,
    directory: BbStr,
    kind: u32,
    extension: BbStr,
    out_name: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let name = app.app.new_item_suggest_in(
            TabId(tab),
            &text_of(directory),
            kind_of(kind),
            &text_of(extension),
        )?;
        unsafe { put(out_name, &name) }
    })
}

/// What creating `name` would produce, without changing anything, as JSON
/// `{directory, name, target, ok, rejected}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_new_item_plan(
    app: *mut BbApp,
    tab: u64,
    directory: BbStr,
    kind: u32,
    name: BbStr,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let json = app.app.new_item_plan_in(
            TabId(tab),
            &text_of(directory),
            &text_of(name),
            kind_of(kind),
        )?;
        unsafe { put(out_json, &json) }
    })
}

/// Creates a folder (`kind` 0) or file (1) in `directory` (empty: the folder the tab shows) and
/// selects it when its listing arrives. Returns the operation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_new_item(
    app: *mut BbApp,
    tab: u64,
    directory: BbStr,
    kind: u32,
    name: BbStr,
    out_op: *mut u64,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let op = app.app.new_item_in(
            TabId(tab),
            &text_of(directory),
            &text_of(name),
            kind_of(kind),
        )?;
        if let Some(out_op) = unsafe { out_op.as_mut() } {
            *out_op = op.0;
        }
        Ok(())
    })
}

/// Bytes in words, the way every size in the app is written ("1.4 MB", three decimals from GB up).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_byte_size(bytes: i64, out_text: *mut BbOwnedStr) -> BbStatus {
    guard(|| unsafe { put(out_text, &bb_core::format::byte_size(bytes)) })
}

/// The size text the folder tree shows beside each of `paths_json` (a JSON array of folder paths), as a
/// JSON array in the same order: "1.2 GB", "1.2 GB *" (part unreadable), or "" when the cache has no
/// size for it - blank, never zero. One indexed lookup; no folder is scanned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tree_sizes(
    app: *mut BbApp,
    paths_json: BbStr,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let paths: Vec<String> = serde_json::from_str(&text_of(paths_json))
            .map_err(|e| bb_app::AppError::Invalid(format!("not a JSON list of paths: {e}")))?;
        let sizes = app.app.tree_sizes(&paths);
        unsafe {
            put(
                out_json,
                &serde_json::to_string(&sizes).unwrap_or_else(|_| "[]".into()),
            )
        }
    })
}

/// The New items page: `{types: [{label, extension, templatePath, enabled}], importing, status}`, the
/// switched-off types included.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_new_types_state(
    app: *mut BbApp,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        unsafe { put(out_json, &app.app.new_types_state_json()) }
    })
}

/// Adds the types Windows offers to the list (read off the UI thread); `SettingsChanged("newFileTypes")`
/// follows when it is done.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_new_types_import(app: *mut BbApp) -> BbStatus {
    guard(|| {
        unsafe { app_of(app)? }.app.new_types_import();
        Ok(())
    })
}

/// The file types the New submenu offers, as JSON `[{label, extension, templatePath}]`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_new_file_types(app: *mut BbApp, out_json: *mut BbOwnedStr) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        unsafe { put(out_json, &app.app.new_file_types_json()) }
    })
}

// ---- the session ----

/// What closing the window would save, as the settings file spells it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_session_capture(
    app: *mut BbApp,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        unsafe { put(out_json, &app.app.session_capture_json()) }
    })
}

/// Writes the sizes a splitter drag left one split's children at (see `App::layout_set_weights`):
/// `path` is `depth` child indexes from the root, `weights` one per child.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_layout_set_weights(
    app: *mut BbApp,
    path: *const u32,
    depth: u32,
    weights: *const f64,
    n: u32,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let path: &[u32] = if depth == 0 || path.is_null() {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(path, depth as usize) }
        };
        let weights: &[f64] = if n == 0 || weights.is_null() {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(weights, n as usize) }
        };
        Ok(app.app.layout_set_weights(path, weights)?)
    })
}

/// The main window's restored bounds in DIPs, and whether it was maximised.
#[repr(C)]
pub struct BbWindowPlacement {
    /// False until a window has closed once: the window opens where Windows puts it.
    pub known: bool,
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
    pub maximized: bool,
}

/// Where the main window was last left.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_window_placement(
    app: *mut BbApp,
    out_placement: *mut BbWindowPlacement,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let out_placement = unsafe { out(out_placement, "out_placement")? };
        let (bounds, maximized) = app.app.window_placement();
        let [left, top, width, height] = bounds.unwrap_or_default();
        *out_placement = BbWindowPlacement {
            known: bounds.is_some(),
            left,
            top,
            width,
            height,
            maximized,
        };
        Ok(())
    })
}

/// Remembers where the main window is (written with the settings at shutdown).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_window_placement_set(
    app: *mut BbApp,
    placement: *const BbWindowPlacement,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let p = unsafe { placement.as_ref() }
            .ok_or_else(|| bb_app::AppError::Invalid("placement".into()))?;
        app.app
            .set_window_placement([p.left, p.top, p.width, p.height], p.maximized);
        Ok(())
    })
}

/// Writes the pane and tab arrangement into the settings now.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_session_save(app: *mut BbApp) -> BbStatus {
    guard(|| Ok(unsafe { app_of(app)? }.app.session_save()?))
}

/// The launch rule: the saved session when asked for and usable, otherwise one tab at the startup
/// folder (or `fallback`). Returns the active tab.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_session_restore(
    app: *mut BbApp,
    fallback: BbStr,
    out_tab: *mut u64,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let tab = app.app.session_restore(&text_of(fallback))?;
        if let Some(out_tab) = unsafe { out_tab.as_mut() } {
            *out_tab = tab.0;
        }
        Ok(())
    })
}

/// Captures, prunes and rebuilds the arrangement in place (the harness's `session`). `out_rebuilt`
/// is false when nothing in it was worth keeping and the arrangement was left alone.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_session_roundtrip(app: *mut BbApp, out_rebuilt: *mut bool) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let rebuilt = app.app.session_roundtrip()?;
        if let Some(out) = unsafe { out_rebuilt.as_mut() } {
            *out = rebuilt;
        }
        Ok(())
    })
}

/// Moves a tab to `index` within its pane (clamped).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_move(app: *mut BbApp, tab: u64, index: u32) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }
            .app
            .move_tab(TabId(tab), index as usize)?)
    })
}

/// Moves a tab into another pane, activating it there; a pane it empties closes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_move_to_pane(app: *mut BbApp, tab: u64, pane: u64) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }
            .app
            .move_tab_to_pane(TabId(tab), PaneId(pane))?)
    })
}

/// Exchanges the active pane's place with the next one's.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_pane_swap(app: *mut BbApp) -> BbStatus {
    guard(|| Ok(unsafe { app_of(app)? }.app.swap_panes()?))
}

/// Gives every pane the same share of the space.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_pane_equalise(app: *mut BbApp) -> BbStatus {
    guard(|| {
        unsafe { app_of(app)? }.app.equalise_panes();
        Ok(())
    })
}

// ---- commands and keys ----

/// Every command in the state a menu would draw it, as JSON
/// `[{id, name, category, icon, gesture, context, owner, whyNot, checked, caption}]`. `whyNot` is
/// **the one gate**: the registry's answer plus "a dialog is up" and "Settings is open".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_command_list(app: *mut BbApp, out_json: *mut BbOwnedStr) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        unsafe { put(out_json, &app.app.command_list_json()) }
    })
}

/// One command's state, as an element of `bb_command_list`. `BB_ERR` for an id nobody knows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_command_state(
    app: *mut BbApp,
    id: BbStr,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let state = app
            .app
            .command_state_json(&text_of(id))
            .ok_or_else(|| Fail::Err("unknown command".into()))?;
        unsafe { put(out_json, &state) }
    })
}

/// Runs a command through the gate. `out_needs_ui` is true when the command is the host's to carry
/// out (a dialog, the clipboard, focus): it was available, and nothing was done.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_command_execute(
    app: *mut BbApp,
    id: BbStr,
    out_needs_ui: *mut bool,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let outcome = app.app.command_execute(&text_of(id))?;
        if let Some(out) = unsafe { out_needs_ui.as_mut() } {
            *out = outcome == bb_app::CommandOutcome::NeedsUi;
        }
        Ok(())
    })
}

fn chord_of(chord: BbKeyChord) -> Option<KeyChord> {
    let mut modifiers = ChordModifiers::NONE;
    if chord.modifiers & 1 != 0 {
        modifiers = modifiers | ChordModifiers::CTRL;
    }
    if chord.modifiers & 2 != 0 {
        modifiers = modifiers | ChordModifiers::ALT;
    }
    if chord.modifiers & 4 != 0 {
        modifiers = modifiers | ChordModifiers::SHIFT;
    }
    KeyChord::create(modifiers, &text_of(chord.key))
}

/// A key press the way the window's one handler takes it: whose it is given what has focus, closing
/// the palette for anything but its own chord, then through the gate. JSON
/// `{claimed, command?, result?: "ran"|"ui"|"unavailable"|"ignored", reason?}`. A claimed chord is
/// the command's even when it cannot run; the host marks the key handled. `repeat` is a held key.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_key_press(
    app: *mut BbApp,
    chord: BbKeyChord,
    focus: BbFocusState,
    repeat: bool,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let Some(chord) = chord_of(chord) else {
            return unsafe { put(out_json, "{\"claimed\":false}") };
        };
        let mut state = FocusState::new(match focus.focus {
            BbFocusKind::BB_FOCUS_FILE_LIST => FocusKind::FileList,
            BbFocusKind::BB_FOCUS_TEXT_EDITABLE => FocusKind::TextEditable,
            BbFocusKind::BB_FOCUS_TEXT_READ_ONLY => FocusKind::TextReadOnly,
            BbFocusKind::BB_FOCUS_OTHER => FocusKind::Other,
        });
        state.in_popup = focus.flags & BB_FOCUS_IN_POPUP != 0;
        state.settings_open = focus.flags & BB_FOCUS_SETTINGS_OPEN != 0;
        state.palette_open = focus.flags & BB_FOCUS_PALETTE_OPEN != 0;
        state.content_blocked = focus.flags & BB_FOCUS_CONTENT_BLOCKED != 0;
        state.recording = focus.flags & BB_FOCUS_RECORDING != 0;
        let json = app.app.key_press(&chord, state, repeat);
        unsafe { put(out_json, &json) }
    })
}

/// Gives a command exactly these shortcuts (texts like `Ctrl+Shift+K`; none leaves it with no key),
/// taking each from whoever had it. Written at once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_keymap_set_chords(
    app: *mut BbApp,
    id: BbStr,
    chords: *const BbStr,
    count: u32,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let chords = unsafe { strings(chords, count) };
        Ok(app.app.keymap_set_chords(&text_of(id), &chords)?)
    })
}

/// Puts one command back to the shortcuts it ships with (`id` empty: all of them).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_keymap_reset(app: *mut BbApp, id: BbStr) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let id = text_of(id);
        if id.is_empty() {
            app.app.keymap_reset_all()?;
        } else {
            app.app.keymap_reset(&id)?;
        }
        Ok(())
    })
}

/// The shortcut a menu prints beside a command (empty when it has none), for text that names a key.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_keymap_gesture(
    app: *mut BbApp,
    id: BbStr,
    out_text: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        unsafe { put(out_text, &app.app.keymap_gesture(&text_of(id))) }
    })
}

/// The Keyboard page: `{filter, selected, recording, finding, capturing, canReassign, text,
/// hasChanged, rows:[{heading, name, id, category, chords, changed, ...}]}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_keys_page(app: *mut BbApp, out_json: *mut BbOwnedStr) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        unsafe { put(out_json, &app.app.keys_page_json()) }
    })
}

/// One verb of the Keyboard page: `filter <text>`, `select <id>`, `change`, `add`, `find`, `cancel`,
/// `press <chord>`, `reassign`, `remove`, `reset`, `reset-all`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_keys_do(app: *mut BbApp, verb: BbStr, arg: BbStr) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }
            .app
            .keys_do(&text_of(verb), &text_of(arg))?)
    })
}

// ---- the palette ----

/// Opens the palette on `seed` (empty: the empty state).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_palette_open(app: *mut BbApp, seed: BbStr) -> BbStatus {
    guard(|| {
        unsafe { app_of(app)? }.app.palette_open(&text_of(seed));
        Ok(())
    })
}

/// `{open, query, hint, empty, selected, rows:[{index, kind: heading|entry|category, text, id, icon,
/// gesture, detail, available, pinned, canPin, canBind, token, count, highlights, selected}]}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_palette_state(app: *mut BbApp, out_json: *mut BbOwnedStr) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        unsafe { put(out_json, &app.app.palette_json()) }
    })
}

/// One verb: `type <text>`, `down [n]`, `up [n]`, `home`, `end`, `select <row>`, `pick` (Enter),
/// `complete` (Tab), `pin`, `shortcut`, `close`. The result is JSON; `pick` answers
/// `{ran, ui?, command?, narrowed?, unavailable?}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_palette_do(
    app: *mut BbApp,
    verb: BbStr,
    arg: BbStr,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let json = app.app.palette_do(&text_of(verb), &text_of(arg))?;
        unsafe { put(out_json, &json) }
    })
}

// ---- the menu bar and toolbars ----

/// Reads the command bars. `what`: `state` (the arrangement in force), `menubar` (the headings),
/// `menu` (arg: a heading's name or position, as it would open now), `toolbar` (arg: `title` or
/// `row1`..., as it shows now), `list` (arg: a list id; the rows it expands to), `page` (the
/// Menus and toolbars page's editor).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_bars_query(
    app: *mut BbApp,
    what: BbStr,
    arg: BbStr,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let arg = text_of(arg);
        let json = match text_of(what).as_str() {
            "state" => app.app.bars_state_json(),
            "menubar" => app.app.bars_menu_bar_json(),
            "menu" => app.app.bars_menu_json(&arg)?,
            "toolbar" => app.app.bars_toolbar_json(&arg)?,
            "list" => app.app.bars_list_rows_json(&arg),
            "page" => app.app.barpage_json(),
            other => return Err(Fail::Err(format!("unknown bars query '{other}'"))),
        };
        unsafe { put(out_json, &json) }
    })
}

/// One verb of the command-bar service: `reset`, `save <name>`, `apply <name>`,
/// `rename <old> to <new>`, `remove <name>`, `set-json <arrangement>`, `menu <caption> <tokens>`
/// (add or replace a top-level menu; tokens are command ids, `-` and `@list`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_bars_do(app: *mut BbApp, verb: BbStr, arg: BbStr) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let (verb, arg) = (text_of(verb), text_of(arg));
        match verb.as_str() {
            "set-json" => app.app.bars_set_json(&arg)?,
            "menu" => {
                let (caption, tokens) = arg.split_once(' ').unwrap_or((&arg, ""));
                app.app.bars_set_menu(caption, tokens)?;
            }
            _ => app.app.bars_do(&verb, &arg)?,
        }
        Ok(())
    })
}

/// Replaces one toolbar's items. `target`: `title` or `row1`...; `tokens`: command ids, `-` for a
/// separator, `@list` for a list; `show_text`: 1 buttons carry their names, 0 they do not, -1 keep.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_bars_set_toolbar(
    app: *mut BbApp,
    target: BbStr,
    tokens: BbStr,
    show_text: i32,
) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }.app.bars_set_toolbar(
            &text_of(target),
            &text_of(tokens),
            show_text,
        )?)
    })
}

/// One verb of the Menus and toolbars page's editor: `select <bar>`, `pick <row>`, `open <row>`,
/// `up`, `add <id|name>`, `add-separator`, `add-menu`, `remove [row]`, `nudge [row] up|down`,
/// `move <from> <to>`, `name <caption>`, `show-text on|off`, `row-show-text on|off`, `add-row`,
/// `remove-row`, `filter <text>`, `unplaced on|off`, `reset`, `apply-saved <name>`,
/// `remove-saved <name>`, `select-saved <name>`, `save <name>`, `rename-saved <old> to <new>`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_barpage_do(app: *mut BbApp, verb: BbStr, arg: BbStr) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }
            .app
            .barpage_do(&text_of(verb), &text_of(arg))?)
    })
}

// ---- columns and the property seam ----

/// The columns a tab shows: `{customized, columns:[{id, header, width, cell, rightAligned,
/// sortable, injected, property, canonical}]}`. `cell` is what `bb_listing_cell` takes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_columns_state(
    app: *mut BbApp,
    tab: u64,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        unsafe { put(out_json, &app.app.columns_json(TabId(tab))?) }
    })
}

/// One verb on a tab's columns: `add <id>`, `remove <id>`, `move <id> <index>`, `width <id> <dips>`,
/// `reset`, `default` (make this the layout new tabs start from), `picked <id>,<id>...`,
/// `order <id>,<id>...` (after a header drag).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_columns_do(
    app: *mut BbApp,
    tab: u64,
    verb: BbStr,
    arg: BbStr,
) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }
            .app
            .columns_do(TabId(tab), &text_of(verb), &text_of(arg))?)
    })
}

/// The column header's right-click checklist: `{builtIns:[{id, header, on, locked}], groups:[{title,
/// items:[...]}], extra:[...]}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_columns_header_menu(
    app: *mut BbApp,
    tab: u64,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        unsafe { put(out_json, &app.app.columns_header_menu_json(TabId(tab))?) }
    })
}

/// The Columns page: the default set new tabs start from, `{columns:[{id, header, width, removable}]}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_columns_defaults(
    app: *mut BbApp,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        unsafe { put(out_json, &app.app.columns_defaults_json()) }
    })
}

/// One edit of that default set: `add <id>`, `remove <id>`, `move <id> <index>`, `width <id> <dips>`,
/// `reset`. Saved at once; every tab that never arranged its own columns follows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_columns_defaults_do(
    app: *mut BbApp,
    verb: BbStr,
    arg: BbStr,
) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }
            .app
            .columns_defaults_do(&text_of(verb), &text_of(arg))?)
    })
}

/// Tab 0 asks about the default set the Columns page edits.
/// What the Add column popup offers for `search`: `{loading, full, groups:[{title, items:[{id,
/// header, detail}]}]}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_columns_candidates(
    app: *mut BbApp,
    tab: u64,
    search: BbStr,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let json = app
            .app
            .columns_candidates_json(TabId(tab), &text_of(search))?;
        unsafe { put(out_json, &json) }
    })
}

/// The properties this machine reported, as JSON `[[canonical, display], ...]`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_columns_set_machine_properties(
    app: *mut BbApp,
    json: BbStr,
) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }
            .app
            .columns_set_machine_properties(&text_of(json))?)
    })
}

/// The shell-property cells the host should read for rows `from..from+count` (count 0: the first
/// 256), as JSON `[[path, canonical], ...]`. Each cell is handed out once. Until the host has asked
/// (and while it owes answers) the tab counts as hydrating. Only meaningful after
/// `bb_ui_set_flag("propertyProvider", true)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_props_requests(
    app: *mut BbApp,
    tab: u64,
    from: u32,
    count: u32,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let json = app
            .app
            .prop_requests(TabId(tab), from as usize, count as usize)?;
        unsafe { put(out_json, &json) }
    })
}

/// The host's answers, as JSON `[{path, canonical, value?: {display, number?, date?}}]`: `date` is
/// UTC FILETIME ticks, and a missing `value` means the handler had none (the cell stays blank and
/// is not asked again).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_props_supply(app: *mut BbApp, tab: u64, json: BbStr) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }
            .app
            .prop_supply_json(TabId(tab), &text_of(json))?)
    })
}

/// The thumbnail view's state: `{scale, isThumbnails, lastScale, aspect:{width, height, ratio,
/// text}}`. The thumbnails themselves are the host's.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_tile_state(
    app: *mut BbApp,
    tab: u64,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        unsafe { put(out_json, &app.app.tile_state_json(TabId(tab))?) }
    })
}

// ---- saved searches, workspaces, bookmarks ----

/// `{searches:[{name, query, scope: current|folder|pc, scopePath, scopeText}], workspaces:[{name,
/// shape, folders}], bookmarks:[{path, name, isDirectory, hidden}], workspacesPlacement,
/// savedSearchesPlacement}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_saved_state(app: *mut BbApp, out_json: *mut BbOwnedStr) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        unsafe { put(out_json, &app.app.saved_json()) }
    })
}

/// One verb. On `tab` (0: none): `save-search <name> [current|folder|pc]` saves what the tab is
/// searching for. Otherwise: `run-search <name>`, `run-search-new-tab <name>`,
/// `rename-search <old> to <new>`, `remove-search <name>`, `save-workspace <name>`,
/// `switch-workspace <name>`, `rename-workspace <old> to <new>`, `remove-workspace <name>`,
/// `toggle-bookmark <path>`, `open-bookmark <here|new-tab|pane-right|pane-below> <path>`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_saved_do(
    app: *mut BbApp,
    tab: u64,
    verb: BbStr,
    arg: BbStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let tab = (tab != 0).then_some(TabId(tab));
        Ok(app.app.saved_do(tab, &text_of(verb), &text_of(arg))?)
    })
}

/// Select (`select` true) or deselect by pattern on `tab`, as JSON `{total, matched, problem,
/// summary, usable}`: what `pattern` picks out of the rows on screen, said as it is typed. With
/// `apply` the matches are added to the selection or taken out of it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_select_pattern(
    app: *mut BbApp,
    tab: u64,
    pattern: BbStr,
    select: bool,
    apply: bool,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let json = app
            .app
            .select_pattern_json(TabId(tab), &text_of(pattern), select, apply)?;
        unsafe { put(out_json, &json) }
    })
}

/// What a Save workspace (`kind` = `workspace`) or Save menus-and-toolbars (`command-bars`) dialog
/// shows for `name`, as JSON `{default, problem, taken}`: the name it opens with, why this one
/// cannot be saved (null when it can), and whether saving replaces one of that name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_save_name(
    app: *mut BbApp,
    kind: BbStr,
    name: BbStr,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let json = app.app.save_name_json(&text_of(kind), &text_of(name))?;
        unsafe { put(out_json, &json) }
    })
}

/// Whether `path` is bookmarked.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_is_bookmarked(app: *mut BbApp, path: BbStr) -> bool {
    let Ok(app) = (unsafe { app_of(app) }) else {
        return false;
    };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.app.is_bookmarked(&text_of(path))
    }))
    .unwrap_or(false)
}

/// Puts `text` on the active tab's status bar: an outcome only the host knows (a copied path).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_status_set(app: *mut BbApp, text: BbStr) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        app.app.set_status(&text_of(text));
        Ok(())
    })
}

/// The name a new saved search (or, with `workspace`, a new workspace) starts with.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_saved_default_name(
    app: *mut BbApp,
    tab: u64,
    workspace: bool,
    out_name: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let name = app.app.saved_default_name(TabId(tab), workspace)?;
        unsafe { put(out_name, &name) }
    })
}

// ---- settings ----

/// Writes what the Settings page still holds. Called on Back and on window close.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_settings_flush(app: *mut BbApp) -> BbStatus {
    guard(|| {
        unsafe { app_of(app)? }.app.settings_flush();
        Ok(())
    })
}

/// `{open, page, pages:[{id, name, icon}], problem}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_settings_page_state(
    app: *mut BbApp,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        unsafe { put(out_json, &app.app.settings_page_json()) }
    })
}

/// Opens the Settings page on `page` (a page id; empty: General). While it is up only app-wide
/// commands run.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_settings_page_open(app: *mut BbApp, page: BbStr) -> BbStatus {
    guard(|| Ok(unsafe { app_of(app)? }.app.settings_open(&text_of(page))?))
}

/// Back: flushes, and refuses with `BB_ERR` (the reason in `bb_last_error`) while a list the page
/// holds is still unwritable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_settings_page_close(app: *mut BbApp) -> BbStatus {
    guard(|| {
        unsafe { app_of(app)? }
            .app
            .settings_close()
            .map_err(Fail::Err)
    })
}

// ---- themes ----

/// `{id, name, isDark, following, lightSlot, darkSlot, appearance:{dark, highContrast},
/// dimmedIconOpacity, available:[{id, name, isDark, builtIn, current}], overridden, dirty, issues}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_theme_state(app: *mut BbApp, out_json: *mut BbOwnedStr) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        unsafe { put(out_json, &app.app.theme_json()) }
    })
}

/// Finds the files under `path` that are byte-for-byte the same (the folder is crawled first, so no index
/// helper is needed). `min_size_bytes` floors the files considered. JSON `{availability, incomplete,
/// wastedBytes, groups:[{sizeBytes, hash, keep:{newest, oldest, shallowest}, files:[{path, name, dir,
/// sizeBytes, modified, hidden, hardlinks}]}]}`; `keep` is the index of the copy each strategy keeps.
/// Blocks: call it from a worker.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_duplicates_scan(
    app: *mut BbApp,
    path: BbStr,
    min_size_bytes: i64,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let json = app
            .app
            .duplicates_scan(&unsafe { path.to_string_lossy() }, min_size_bytes)?;
        unsafe { put(out_json, &json) }
    })
}

/// The search syntax help, grouped: `[{title, blurb, entries:[{example, meaning}]}]`. The same table
/// the parser reads, so the help cannot advertise a key the parser does not implement.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_search_syntax_help(out_json: *mut BbOwnedStr) -> BbStatus {
    guard(|| unsafe { put(out_json, &bb_app::App::search_syntax_json()) })
}

/// Every colour token, for the theme editor: `[{key, group, name, description, core, argb,
/// overridden}]` in editor display order.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_theme_tokens(app: *mut BbApp, out_json: *mut BbOwnedStr) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        unsafe { put(out_json, &app.app.theme_tokens_json()) }
    })
}

/// The host tells us what Windows looks like now. Following Windows swaps the theme (and raises
/// `BB_EVT_MODEL_CHANGED` 2: re-push the palette); high contrast wins over both slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_theme_set_system_appearance(
    app: *mut BbApp,
    is_dark: bool,
    high_contrast: bool,
) -> BbStatus {
    guard(|| {
        unsafe { app_of(app)? }
            .app
            .theme_set_system_appearance(is_dark, high_contrast);
        Ok(())
    })
}

/// One verb: `select <id>`, `follow on|off`, `slot light|dark <id>`, `override <token> <#hex>`,
/// `clear-override <token>`, `persist`, `revert`, `reset-all`, `save-as <name>`, `import <path>`,
/// `delete <id>`, `export <path>`. `save-as` and `import` answer the new theme's id in `out_result`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_theme_do(
    app: *mut BbApp,
    verb: BbStr,
    arg: BbStr,
    out_result: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let result = app.app.theme_do(&text_of(verb), &text_of(arg))?;
        if out_result.is_null() {
            Ok(())
        } else {
            unsafe { put(out_result, &result) }
        }
    })
}

// ---- the search index ----

/// `{status, canRetry, canStart, presence, anyIndexed, building, banner:{show, message, canStart}}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_index_state(app: *mut BbApp, out_json: *mut BbOwnedStr) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        unsafe { put(out_json, &app.app.index_json()) }
    })
}

/// `dismiss-banner` ("not now": not persisted), `start` (the one thing that raises the elevation
/// prompt), `retry`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_index_do(app: *mut BbApp, verb: BbStr) -> BbStatus {
    guard(|| Ok(unsafe { app_of(app)? }.app.index_do(&text_of(verb))?))
}

// ---- what the host tells us about the window ----

/// Tells us a fact about the window that decides what can run: `clipboardHasFiles`,
/// `previewHasMedia`, `propertyProvider` (the host reads shell properties for property columns).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_ui_set_flag(app: *mut BbApp, name: BbStr, value: bool) -> BbStatus {
    guard(|| {
        unsafe { app_of(app)? }
            .app
            .ui_set_flag(&text_of(name), value);
        Ok(())
    })
}

/// A modal dialog opened (`true`) or closed. While one is up nothing about the folders can run.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_ui_set_dialog_open(app: *mut BbApp, open: bool) -> BbStatus {
    guard(|| {
        unsafe { app_of(app)? }.app.ui_set_dialog_open(open);
        Ok(())
    })
}

/// Starts or stops comparing the active pane with the next. (Only the switch and its gates live
/// here; the comparison engine is not part of this layer.)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_compare_toggle(app: *mut BbApp) -> BbStatus {
    guard(|| Ok(unsafe { app_of(app)? }.app.compare_toggle()?))
}
