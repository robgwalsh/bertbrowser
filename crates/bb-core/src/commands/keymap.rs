//! Everything about shortcuts that can be decided without a keyboard: which chords are in force,
//! which may be bound at all, and whether a chord fires given what has focus. The dispatcher sees a
//! key *before* the focused control does, so the protection a text box used to get for free (the
//! box handled Backspace and the binding never heard of it) is a rule here, about the *chord* rather
//! than the command, because once chords can be rebound nobody can say which command will be
//! sitting on Backspace. Spec: Core/Services/Commands/KeymapRules.cs.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::{ChordKeyKind, ChordModifiers, CommandCatalog, CommandContext, KeyChord};

/// Something a shortcut can be bound to: a catalogue command, or one of the user's own.
#[derive(Clone, Debug)]
pub struct Bindable {
    pub id: String,
    pub context: CommandContext,
    pub defaults: Vec<KeyChord>,
}

impl Bindable {
    pub fn new(id: &str, context: CommandContext, defaults: &[&str]) -> Bindable {
        Bindable {
            id: id.to_string(),
            context,
            defaults: defaults.iter().map(|c| KeyChord::parse(c)).collect(),
        }
    }
}

/// The user's changes: command id to the chord texts it has, an empty list meaning "no shortcut".
pub type Overrides = BTreeMap<String, Vec<String>>;

/// What has the keyboard, as far as a shortcut needs to know.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FocusKind {
    FileList,
    /// A box somebody is typing in: search, path, a settings field.
    TextEditable,
    /// Selectable text nobody can type in: the preview pane's.
    TextReadOnly,
    /// Anything else: the folder tree, a tab, a button.
    Other,
}

impl FocusKind {
    pub const ALL: [FocusKind; 4] = [
        FocusKind::FileList,
        FocusKind::TextEditable,
        FocusKind::TextReadOnly,
        FocusKind::Other,
    ];
}

/// The state of the window at the moment a key arrives. `content_blocked`: a modal dialog is up but
/// the window still receives keys behind it. `recording`: the Keyboard page is waiting for a chord.
/// `in_popup`: a menu or dropdown has the keyboard and its keys still travel through the window.
#[derive(Clone, Copy, Debug)]
pub struct FocusState {
    pub focus: FocusKind,
    pub settings_open: bool,
    pub palette_open: bool,
    pub content_blocked: bool,
    pub recording: bool,
    pub in_popup: bool,
}

impl FocusState {
    pub fn new(focus: FocusKind) -> FocusState {
        FocusState {
            focus,
            settings_open: false,
            palette_open: false,
            content_blocked: false,
            recording: false,
            in_popup: false,
        }
    }
}

/// The shortcuts in force: the shipped defaults with the user's changes over them. Each chord belongs
/// to at most one command.
pub struct Keymap {
    chords: HashMap<String, Vec<KeyChord>>,
    owners: HashMap<KeyChord, String>,
    contexts: HashMap<String, CommandContext>,
}

impl Keymap {
    /// The chords that run this command, the one menus print first.
    pub fn chords_for(&self, id: &str) -> &[KeyChord] {
        self.chords.get(id).map_or(&[], Vec::as_slice)
    }

    /// The command a chord belongs to, wherever it happens to be live.
    pub fn owner_of(&self, chord: &KeyChord) -> Option<&str> {
        self.owners.get(chord).map(String::as_str)
    }

    pub fn context_of(&self, id: &str) -> Option<CommandContext> {
        self.contexts.get(id).copied()
    }

    /// What a menu or tooltip prints beside the command: its first chord, or nothing.
    pub fn gesture_text(&self, id: &str) -> String {
        self.chords_for(id)
            .first()
            .map(ToString::to_string)
            .unwrap_or_default()
    }
}

pub struct KeymapRules;

impl KeymapRules {
    /// The catalogue's commands as the keymap sees them.
    pub fn catalog_bindables() -> Vec<Bindable> {
        CommandCatalog::all()
            .iter()
            .map(|c| Bindable {
                id: c.id.to_string(),
                context: c.context,
                defaults: c.default_chords.clone(),
            })
            .collect()
    }

    /// The keymap in force. An override replaces a command's chords outright (an empty list takes a
    /// default away). Where two commands claim one chord, the user's choice beats a default and an
    /// earlier command beats a later one; the loser simply does not have it.
    pub fn resolve(bindables: &[Bindable], overrides: Option<&Overrides>) -> Keymap {
        // One entry per id, the first: two of the user's commands sharing an id is a hand-edited
        // settings file, and it must cost one of them its shortcut rather than the app its startup.
        let mut seen = HashSet::new();
        let all: Vec<&Bindable> = bindables
            .iter()
            .filter(|b| seen.insert(b.id.as_str()))
            .collect();
        let mut owners: HashMap<KeyChord, String> = HashMap::new();
        let mut claimed: HashMap<String, Vec<KeyChord>> = HashMap::new();
        let contexts: HashMap<String, CommandContext> =
            all.iter().map(|b| (b.id.clone(), b.context)).collect();

        // Two passes, so a chord the user gave to one command is taken before another command's
        // default can claim it.
        for overridden in [true, false] {
            for bindable in &all {
                let saved = overrides.and_then(|o| o.get(&bindable.id));
                if saved.is_some() != overridden {
                    continue;
                }
                let wanted: Vec<KeyChord> = match saved {
                    None => bindable.defaults.clone(),
                    Some(texts) => texts
                        .iter()
                        .filter_map(|t| KeyChord::try_parse(t))
                        .filter(|c| Self::refuse(c, bindable.context).is_none())
                        .collect(),
                };
                let mut mine = Vec::new();
                for chord in wanted {
                    if let std::collections::hash_map::Entry::Vacant(e) = owners.entry(chord) {
                        e.insert(bindable.id.clone());
                        mine.push(chord);
                    }
                }
                claimed.insert(bindable.id.clone(), mine);
            }
        }
        Keymap {
            chords: claimed,
            owners,
            contexts,
        }
    }

    /// Why this chord may not be bound to a command of this context, or None when it may. These are
    /// the keys that already mean something to whatever has focus.
    pub fn refuse(chord: &KeyChord, context: CommandContext) -> Option<String> {
        let bare = !chord.has(ChordModifiers::CTRL) && !chord.has(ChordModifiers::ALT);
        if chord.modifiers() == ChordModifiers::ALT && chord.key() == "F4" {
            return Some("Alt+F4 closes the window.".into());
        }
        if chord.has(ChordModifiers::ALT)
            && !chord.has(ChordModifiers::CTRL)
            && chord.key() == "Space"
        {
            return Some("Alt+Space opens the window menu.".into());
        }
        if !bare {
            return None;
        }
        match chord.kind() {
            ChordKeyKind::Function => None,
            // A letter on its own is type-ahead in a file list and typing everywhere else.
            ChordKeyKind::Printable => Some("A key that types a character needs Ctrl or Alt with it.".into()),
            // Num+ / Num- selecting by pattern is what every commander does with them.
            ChordKeyKind::Numpad => (context != CommandContext::FileList)
                .then(|| "A number-pad key on its own can only be given to a command that acts on the file list.".into()),
            ChordKeyKind::Caret => {
                Some("The arrow and paging keys move through lists, so they need Ctrl or Alt with them.".into())
            }
            ChordKeyKind::Editing => {
                // Enter, Backspace, Del and Insert are free in a file list. Tab and Esc are not free
                // anywhere.
                if chord.key() == "Tab" || chord.key() == "Esc" {
                    return Some(format!("{} is used to move around the window.", chord.key()));
                }
                if chord.key() == "Backspace" {
                    return None;
                }
                (context != CommandContext::FileList).then(|| {
                    format!("{} on its own can only be given to a command that acts on the file list.", chord.key())
                })
            }
        }
    }

    /// Whether a chord may fire while somebody is typing. Anything the text box would use itself does
    /// not: plain and shifted keys, the clipboard and undo chords, word-wise caret movement, and
    /// Ctrl+Alt with a printable key, which is AltGr.
    pub fn fires_in_editable_text(chord: &KeyChord) -> bool {
        let ctrl = chord.has(ChordModifiers::CTRL);
        let alt = chord.has(ChordModifiers::ALT);
        if !ctrl && !alt {
            return chord.kind() == ChordKeyKind::Function;
        }
        if is_text_command(chord) {
            return false;
        }
        if ctrl && alt {
            return !matches!(chord.kind(), ChordKeyKind::Printable | ChordKeyKind::Numpad);
        }
        // Ctrl+Left, Ctrl+Shift+End, Ctrl+Backspace, Ctrl+Insert: the box's own.
        if ctrl
            && !alt
            && (chord.kind() == ChordKeyKind::Caret
                || matches!(chord.key(), "Backspace" | "Del" | "Insert"))
        {
            return false;
        }
        true
    }

    /// Whether a chord may fire over selectable read-only text. Selecting, copying and moving the
    /// caret still belong to the text, and undo stays off as it always was over the preview.
    pub fn fires_in_read_only_text(chord: &KeyChord) -> bool {
        let ctrl = chord.has(ChordModifiers::CTRL);
        let alt = chord.has(ChordModifiers::ALT);
        if is_text_command(chord) {
            return false;
        }
        if ctrl && alt {
            return !matches!(chord.kind(), ChordKeyKind::Printable | ChordKeyKind::Numpad);
        }
        if !alt && chord.kind() == ChordKeyKind::Caret {
            return false;
        }
        true
    }

    /// The command this key press runs, or None when it is nobody's here and must carry on to the
    /// focused control. Says nothing about whether the command is *available*: a claimed chord is
    /// the command's even when it can do nothing, so that it never falls through to type-ahead.
    pub fn dispatch(keymap: &Keymap, chord: &KeyChord, state: &FocusState) -> Option<String> {
        if state.content_blocked || state.recording || state.in_popup {
            return None;
        }
        let id = keymap.owner_of(chord)?;
        let context = keymap.context_of(id)?;
        // While the palette is up its own box has the keyboard, and the only shortcuts that still
        // mean anything are the ones that would work from anywhere.
        if (state.palette_open || state.settings_open) && context != CommandContext::App {
            return None;
        }
        if context == CommandContext::FileList && state.focus != FocusKind::FileList {
            return None;
        }
        match state.focus {
            FocusKind::TextEditable if !Self::fires_in_editable_text(chord) => None,
            FocusKind::TextReadOnly if !Self::fires_in_read_only_text(chord) => None,
            _ => Some(id.to_string()),
        }
    }

    /// Whether a bare Alt or F10 may hand the keyboard to the menu bar. Not under a dialog, not while
    /// the Keyboard page waits for a chord, and not with the palette up.
    pub fn allows_menu_mode(state: &FocusState) -> bool {
        !state.content_blocked && !state.recording && !state.palette_open
    }

    /// The overrides after giving `id` exactly these chords, taking each away from whichever command
    /// held it. An entry equal to its defaults is dropped, so the stored map stays "what the user
    /// changed" and a later build's new default still arrives.
    pub fn with_chords(
        bindables: &[Bindable],
        overrides: Option<&Overrides>,
        id: &str,
        chords: &[KeyChord],
    ) -> Overrides {
        let mut result = overrides.cloned().unwrap_or_default();
        let mut keymap = Self::resolve(bindables, Some(&result));
        for chord in chords {
            let Some(owner) = keymap.owner_of(chord).map(str::to_string) else {
                continue;
            };
            if owner == id {
                continue;
            }
            let remaining: Vec<KeyChord> = keymap
                .chords_for(&owner)
                .iter()
                .filter(|c| *c != chord)
                .copied()
                .collect();
            store(bindables, &mut result, &owner, &remaining);
            keymap = Self::resolve(bindables, Some(&result));
        }
        let mut distinct = Vec::new();
        for c in chords {
            if !distinct.contains(c) {
                distinct.push(*c);
            }
        }
        store(bindables, &mut result, id, &distinct);
        result
    }

    /// The overrides after putting this command back to the shortcuts it ships with, taking each back
    /// from any command it had been given to.
    pub fn with_defaults(
        bindables: &[Bindable],
        overrides: Option<&Overrides>,
        id: &str,
    ) -> Overrides {
        let defaults = bindables
            .iter()
            .find(|b| b.id == id)
            .map(|b| b.defaults.clone())
            .unwrap_or_default();
        Self::with_chords(bindables, overrides, id, &defaults)
    }

    /// Whether holding a command's key down should run it again and again. Almost nothing should: a
    /// held Ctrl+W closing every tab is what auto-repeat does to a command that was only ever meant
    /// once. These few are steps.
    pub fn repeats_when_held(id: &str) -> bool {
        matches!(
            id,
            "tab.next"
                | "tab.previous"
                | "tab.move-left"
                | "tab.move-right"
                | "pane.next"
                | "pane.previous"
                | "view.thumbs-larger"
                | "view.thumbs-smaller"
        )
    }
}

fn store(bindables: &[Bindable], result: &mut Overrides, target: &str, wanted: &[KeyChord]) {
    let defaults = bindables
        .iter()
        .find(|b| b.id == target)
        .map(|b| b.defaults.as_slice())
        .unwrap_or(&[]);
    if wanted == defaults {
        result.remove(target);
    } else {
        result.insert(
            target.to_string(),
            wanted.iter().map(ToString::to_string).collect(),
        );
    }
}

/// Ctrl+A/C/X/V/Z/Y and Ctrl+Shift+Z: select, clipboard, undo, a text control's own.
fn is_text_command(chord: &KeyChord) -> bool {
    let m = chord.modifiers();
    (m == ChordModifiers::CTRL && matches!(chord.key(), "A" | "C" | "X" | "V" | "Z" | "Y"))
        || (m == ChordModifiers::CTRL | ChordModifiers::SHIFT && chord.key() == "Z")
        || (m == ChordModifiers::SHIFT && matches!(chord.key(), "Del" | "Insert"))
}
