//! Keys and commands.

use super::*;

// ---- keys and commands ----

/// A key press, translated by the UI: `modifiers` is a mask (1 Ctrl, 2 Alt, 4 Shift) and `key` the
/// key's name in this app's vocabulary (`P`, `F7`, `Del`, `Num+`). Rust decides what it means.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BbKeyChord {
    pub modifiers: u32,
    pub key: BbStr,
}

/// What has the keyboard, as far as a shortcut needs to know.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BbFocusKind {
    BB_FOCUS_FILE_LIST = 0,
    /// A box somebody is typing in.
    BB_FOCUS_TEXT_EDITABLE = 1,
    /// Selectable text nobody can type in.
    BB_FOCUS_TEXT_READ_ONLY = 2,
    BB_FOCUS_OTHER = 3,
}

/// `BbFocusState.flags` bits.
pub const BB_FOCUS_SETTINGS_OPEN: u32 = 1;
pub const BB_FOCUS_PALETTE_OPEN: u32 = 2;
/// A modal dialog is up but the window still receives keys behind it.
pub const BB_FOCUS_CONTENT_BLOCKED: u32 = 4;
/// The Keyboard page is waiting for a chord.
pub const BB_FOCUS_RECORDING: u32 = 8;
/// A menu or dropdown has the keyboard.
pub const BB_FOCUS_IN_POPUP: u32 = 16;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BbFocusState {
    pub focus: BbFocusKind,
    pub flags: u32,
}

/// Which command, if any, owns this key press in this state. `out_command` is NULL when it is nobody's
/// and must carry on to the focused control. Says nothing about whether the command is available: a
/// claimed chord is the command's even when it can do nothing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_key_dispatch(
    app: *mut BbApp,
    chord: BbKeyChord,
    focus: BbFocusState,
    out_command: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let out_command = out(out_command, "out_command")?;
        *out_command = BbOwnedStr::none();
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
        let key = chord.key.to_string_lossy();
        // A key this app has no name for cannot be bound, so it is nobody's.
        let Some(chord) = KeyChord::create(modifiers, &key) else {
            return Ok(());
        };
        let mut state = FocusState::new(match focus.focus {
            BbFocusKind::BB_FOCUS_FILE_LIST => FocusKind::FileList,
            BbFocusKind::BB_FOCUS_TEXT_EDITABLE => FocusKind::TextEditable,
            BbFocusKind::BB_FOCUS_TEXT_READ_ONLY => FocusKind::TextReadOnly,
            BbFocusKind::BB_FOCUS_OTHER => FocusKind::Other,
        });
        state.settings_open = focus.flags & BB_FOCUS_SETTINGS_OPEN != 0;
        state.palette_open = focus.flags & BB_FOCUS_PALETTE_OPEN != 0;
        state.content_blocked = focus.flags & BB_FOCUS_CONTENT_BLOCKED != 0;
        state.recording = focus.flags & BB_FOCUS_RECORDING != 0;
        state.in_popup = focus.flags & BB_FOCUS_IN_POPUP != 0;
        if let Some(id) = app.app.key_dispatch(&chord, &state) {
            *out_command = BbOwnedStr::from_str(&id);
        }
        Ok(())
    })
}

/// Runs a command against the active tab. Fails with the reason `bb_command_why_not` gives.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_command_run(app: *mut BbApp, id: BbStr) -> BbStatus {
    guard(|| Ok(app_of(app)?.app.command_run(&id.to_string_lossy())?))
}

/// Why a command cannot run now, in words fit to show beside it; `out_reason` is NULL when it can.
/// `BB_ERR` for an id nobody knows. This is the one gate a menu row, a key and the palette share.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_command_why_not(
    app: *mut BbApp,
    id: BbStr,
    out_reason: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let out_reason = out(out_reason, "out_reason")?;
        *out_reason = app
            .app
            .command_why_not(&id.to_string_lossy())?
            .map_or_else(BbOwnedStr::none, |r| BbOwnedStr::from_str(&r));
        Ok(())
    })
}
