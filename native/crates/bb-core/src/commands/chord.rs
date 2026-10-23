//! One keyboard shortcut, modifiers and a key, as the keymap stores it and the menus print it. The
//! key is a name of this crate's own (`P`, `F7`, `Del`, `Num+`); the UI converts a key event to one
//! of these and nothing here ever sees a virtual key. The text form is the persisted form:
//! `Ctrl+Shift+P`, modifiers in that fixed order. Spec: Core/Services/Commands/KeyChord.cs.

use std::fmt;
use std::ops::BitOr;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct ChordModifiers(u8);

impl ChordModifiers {
    pub const NONE: ChordModifiers = ChordModifiers(0);
    pub const CTRL: ChordModifiers = ChordModifiers(1);
    pub const ALT: ChordModifiers = ChordModifiers(2);
    pub const SHIFT: ChordModifiers = ChordModifiers(4);

    pub fn has(self, other: ChordModifiers) -> bool {
        self.0 & other.0 != 0
    }
}

impl BitOr for ChordModifiers {
    type Output = ChordModifiers;
    fn bitor(self, rhs: ChordModifiers) -> ChordModifiers {
        ChordModifiers(self.0 | rhs.0)
    }
}

/// What a key does when nothing is bound to it, which is what decides where a chord built on it may
/// fire.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ChordKeyKind {
    /// Types a character: letters, digits, punctuation, Space.
    Printable,
    /// F1 to F24: never typed, never navigation.
    Function,
    /// Enter, Backspace, Del, Insert, Tab, Esc.
    Editing,
    /// The arrows, Home, End, PageUp, PageDown.
    Caret,
    /// The number pad's digits and operators.
    Numpad,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct KeyChord {
    modifiers: ChordModifiers,
    key: &'static str,
    kind: ChordKeyKind,
}

const LETTERS: [&str; 26] = [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S",
    "T", "U", "V", "W", "X", "Y", "Z",
];
const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
const PUNCTUATION: [&str; 11] = [",", ".", "/", ";", "'", "[", "]", "\\", "-", "=", "`"];
const FUNCTION: [&str; 24] = [
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14", "F15",
    "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
];
const NUMPAD_DIGITS: [&str; 10] = [
    "Num0", "Num1", "Num2", "Num3", "Num4", "Num5", "Num6", "Num7", "Num8", "Num9",
];

/// Named keys with their aliases.
const NAMED: &[(&str, ChordKeyKind, &[&str])] = &[
    ("Space", ChordKeyKind::Printable, &[]),
    ("Enter", ChordKeyKind::Editing, &["Return"]),
    ("Backspace", ChordKeyKind::Editing, &["Back"]),
    ("Del", ChordKeyKind::Editing, &["Delete"]),
    ("Insert", ChordKeyKind::Editing, &["Ins"]),
    ("Tab", ChordKeyKind::Editing, &[]),
    ("Esc", ChordKeyKind::Editing, &["Escape"]),
    ("Left", ChordKeyKind::Caret, &[]),
    ("Right", ChordKeyKind::Caret, &[]),
    ("Up", ChordKeyKind::Caret, &[]),
    ("Down", ChordKeyKind::Caret, &[]),
    ("Home", ChordKeyKind::Caret, &[]),
    ("End", ChordKeyKind::Caret, &[]),
    ("PageUp", ChordKeyKind::Caret, &["PgUp"]),
    ("PageDown", ChordKeyKind::Caret, &["PgDn"]),
    ("Num+", ChordKeyKind::Numpad, &[]),
    ("Num-", ChordKeyKind::Numpad, &[]),
    ("Num*", ChordKeyKind::Numpad, &[]),
    ("Num/", ChordKeyKind::Numpad, &[]),
    ("Num.", ChordKeyKind::Numpad, &[]),
];

/// The canonical name and kind of a key, looked up ignoring case.
fn lookup(key: &str) -> Option<(&'static str, ChordKeyKind)> {
    let find = |table: &[&'static str]| table.iter().find(|k| k.eq_ignore_ascii_case(key)).copied();
    if let Some(k) = find(&LETTERS)
        .or_else(|| find(&DIGITS))
        .or_else(|| find(&PUNCTUATION))
    {
        return Some((k, ChordKeyKind::Printable));
    }
    if let Some(k) = find(&FUNCTION) {
        return Some((k, ChordKeyKind::Function));
    }
    if let Some(k) = find(&NUMPAD_DIGITS) {
        return Some((k, ChordKeyKind::Numpad));
    }
    for (name, kind, aliases) in NAMED {
        if name.eq_ignore_ascii_case(key) || aliases.iter().any(|a| a.eq_ignore_ascii_case(key)) {
            return Some((name, *kind));
        }
    }
    None
}

impl KeyChord {
    pub fn modifiers(&self) -> ChordModifiers {
        self.modifiers
    }

    /// The key's canonical name.
    pub fn key(&self) -> &'static str {
        self.key
    }

    pub fn kind(&self) -> ChordKeyKind {
        self.kind
    }

    pub fn has(&self, modifier: ChordModifiers) -> bool {
        self.modifiers.has(modifier)
    }

    /// Whether `key` names a key a chord can be built on.
    pub fn is_key(key: &str) -> bool {
        lookup(key).is_some()
    }

    /// A chord from its parts, or None when the key is not one a chord can use.
    pub fn create(modifiers: ChordModifiers, key: &str) -> Option<KeyChord> {
        lookup(key).map(|(name, kind)| KeyChord {
            modifiers,
            key: name,
            kind,
        })
    }

    /// Reads `Ctrl+Shift+P`, in any case and any modifier order. Modifiers come off the front, and
    /// only while something is left after them, so `Ctrl+=` and `Num+` read as a key rather than as
    /// a dangling separator.
    pub fn try_parse(text: &str) -> Option<KeyChord> {
        let mut rest = text.trim();
        let mut modifiers = ChordModifiers::NONE;
        loop {
            if strip(&mut rest, "Ctrl+") || strip(&mut rest, "Control+") {
                modifiers = modifiers | ChordModifiers::CTRL;
            } else if strip(&mut rest, "Alt+") {
                modifiers = modifiers | ChordModifiers::ALT;
            } else if strip(&mut rest, "Shift+") {
                modifiers = modifiers | ChordModifiers::SHIFT;
            } else {
                break;
            }
        }
        KeyChord::create(modifiers, rest)
    }

    /// As [`try_parse`](Self::try_parse), for text known to be a chord (the catalogue's own).
    pub fn parse(text: &str) -> KeyChord {
        KeyChord::try_parse(text).unwrap_or_else(|| panic!("'{text}' is not a keyboard shortcut."))
    }
}

fn strip(text: &mut &str, prefix: &str) -> bool {
    if text.len() <= prefix.len() || !text[..prefix.len()].eq_ignore_ascii_case(prefix) {
        return false;
    }
    *text = &text[prefix.len()..];
    true
}

impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.has(ChordModifiers::CTRL) {
            f.write_str("Ctrl+")?;
        }
        if self.has(ChordModifiers::ALT) {
            f.write_str("Alt+")?;
        }
        if self.has(ChordModifiers::SHIFT) {
            f.write_str("Shift+")?;
        }
        f.write_str(self.key)
    }
}
