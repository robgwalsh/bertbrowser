//! The decisions behind hosting other programs' context-menu entries: which registrations become
//! one extension, which are never shown, what the user has hidden, and how the shell's menu text
//! becomes a header.
//!
//! The app composes the menu itself, one contributor at a time, rather than asking the shell for
//! its finished default menu. The finished one mixes in Cut, Copy, Delete, Rename, Properties,
//! Send to and Share — all of which this app already has — and gives no way to tell which handler
//! produced an item. Composing per handler is what makes "hide this one" possible at all, and it
//! is also why no stock verb needs filtering by its (localised) text.
//! Spec: Core/Services/ShellMenu/ShellMenuRules.cs.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use bb_paths::fold_upper;

use super::{
    Guid, ShellExtension, ShellExtensionKind, ShellHandlerRegistration, ShellMenuEntry,
    ShellVerbRegistration, eq_ignore_case, fold,
};

/// Windows' own handlers for things this app already has — Open with, Send to, Share, Give access
/// to, Restore previous versions, Copy as path, the pin-to and library verbs, Extract All on a zip.
/// They register under `ContextMenuHandlers` exactly as 7-Zip does, so without this list the menu
/// would offer Explorer's stock items twice over.
static STOCK_HANDLERS: LazyLock<HashSet<Guid>> = LazyLock::new(|| {
    [
        "09799AFB-AD67-11D1-ABCD-00C04FC30936", // Open With
        "7BA4C740-9E81-11CF-99D3-00AA004AE837", // Send To
        "F81E9010-6EA4-11CE-A7FF-00AA003CA9F6", // Sharing ("Give access to")
        "E2BF9676-5F8F-435C-97EB-11607A5BEDF7", // Modern Sharing ("Share")
        "596AB062-B4D2-4215-9F74-E9109B0A8153", // Previous Versions
        "F3D06E7C-1E45-4A26-847E-F9FCDEE59BE0", // Copy As Path
        "B455F46E-E4AF-4035-B0A4-CF18D2F6F28E", // Pin to Quick access
        "470C0EBD-5D73-4D58-9CED-E91E22E23282", // Pin to Start
        "90AA3A4E-1CBA-4233-B8BB-535773D48449", // Pin to taskbar
        "3DAD6C5D-2167-4CAE-9914-F99E41C12CFA", // Include in library
        "7AD84985-87B4-4A16-BE58-8B72A5B390F7", // Play To ("Cast to Device")
        "B8CDCB65-B1BF-4B42-9428-1DFDB7EE92AF", // Compressed Folder ("Extract All…")
        "BD472F60-27FA-11CF-B8B4-444553540000", // Compressed Folder, the older registration
        "D6791A63-E7E2-4FEE-BF52-5DED8E86E9B8", // Portable Devices
    ]
    .iter()
    .map(|g| Guid::parse(g).expect("a stock handler CLSID"))
    .collect()
});

pub struct ShellMenuRules;

impl ShellMenuRules {
    /// Static verbs never offered, because BertBrowser has its own item for what they do or
    /// because Windows itself supplies them everywhere. `edit` and `print` are deliberately absent:
    /// they are a file type's own verbs, Explorer shows them, and the app has nothing for them.
    pub const HIDDEN_VERBS: [&'static str; 13] = [
        "open",
        "explore",
        "opennewwindow",
        "opennewprocess",
        "opencontaining",
        "find",
        "runas",
        "runasuser",
        "cmd",
        "powershell",
        "pintohome",
        "pintostartscreen",
        "printto",
    ];

    pub fn is_hidden_verb(verb: &str) -> bool {
        Self::HIDDEN_VERBS.iter().any(|h| eq_ignore_case(h, verb))
    }

    pub fn stock_handlers() -> &'static HashSet<Guid> {
        &STOCK_HANDLERS
    }

    /// Merges raw registrations into extensions: one per CLSID, one per verb name, in the order
    /// Explorer shows them — static verbs first, then handlers, each as first registered.
    /// `blocked` is the CLSIDs Windows itself refuses to load (the `Shell Extensions\Blocked`
    /// list), honoured for the same reason Explorer honours it; the stock handlers are left out
    /// as well.
    pub fn catalog(
        handlers: &[ShellHandlerRegistration],
        verbs: &[ShellVerbRegistration],
        blocked: &HashSet<Guid>,
    ) -> Vec<ShellExtension> {
        let mut rows: Vec<ShellExtension> = Vec::new();
        let mut by_id: HashMap<String, usize> = HashMap::new();

        for verb in verbs {
            if !is_offerable(verb) {
                continue;
            }

            let id = ShellExtension::verb_id(&verb.verb);
            if let Some(&index) = by_id.get(&id) {
                let existing = &mut rows[index];
                // A later family may be the one that carries the display name.
                if existing.name.is_empty()
                    && let Some(name) = &verb.display_name
                {
                    existing.name = name.clone();
                }
                existing.verbs.push(verb.clone());
                append_family(&mut existing.families, &verb.family);
                continue;
            }

            by_id.insert(id.clone(), rows.len());
            rows.push(ShellExtension {
                id,
                name: verb.display_name.clone().unwrap_or_default(),
                kind: ShellExtensionKind::Verb,
                clsid: None,
                verbs: vec![verb.clone()],
                families: vec![verb.family.clone()],
            });
        }

        for handler in handlers {
            if blocked.contains(&handler.clsid) || STOCK_HANDLERS.contains(&handler.clsid) {
                continue;
            }

            let id = ShellExtension::handler_id(handler.clsid);
            if let Some(&index) = by_id.get(&id) {
                append_family(&mut rows[index].families, &handler.family);
                continue;
            }

            by_id.insert(id.clone(), rows.len());
            rows.push(ShellExtension {
                id,
                name: handler_name(handler),
                kind: ShellExtensionKind::Handler,
                clsid: Some(handler.clsid),
                verbs: vec![],
                families: vec![handler.family.clone()],
            });
        }

        // A verb whose every registration lacked a name falls back to the verb itself, once all of
        // them have been seen — capitalised, since a canonical verb ("print") is written lowercase
        // and Explorer shows it from its own string table.
        for row in &mut rows {
            if row.name.is_empty()
                && let Some(first) = row.verbs.first()
            {
                row.name = capitalize(&first.verb);
            }
        }

        rows
    }

    /// The catalog minus the master switch and the user's hidden set.
    pub fn visible<S: AsRef<str>>(
        catalog: &[ShellExtension],
        enabled: bool,
        hidden_ids: impl IntoIterator<Item = S>,
    ) -> Vec<&ShellExtension> {
        if !enabled {
            return vec![];
        }
        let hidden: HashSet<String> = hidden_ids
            .into_iter()
            .map(|h| fold_upper(h.as_ref()))
            .collect();
        catalog
            .iter()
            .filter(|e| !hidden.contains(&fold_upper(&e.id)))
            .collect()
    }

    /// The hidden list to store after the Settings page is saved. An id hidden earlier but not in
    /// today's catalog stays hidden — an uninstalled extension must not quietly reappear when it is
    /// reinstalled, and the page cannot have shown a row for something it never found.
    pub fn hidden_after_save<P: AsRef<str>, R: AsRef<str>>(
        previously_hidden: impl IntoIterator<Item = P>,
        rows: &[(R, bool)],
    ) -> Vec<String> {
        let seen: HashSet<String> = rows.iter().map(|(id, _)| fold_upper(id.as_ref())).collect();

        let mut hidden: Vec<String> = Vec::new();
        let mut taken: HashSet<String> = HashSet::new();

        for id in previously_hidden {
            let id = id.as_ref();
            if !seen.contains(&fold_upper(id)) && taken.insert(fold_upper(id)) {
                hidden.push(id.to_string());
            }
        }

        for (id, is_shown) in rows {
            let id = id.as_ref();
            if !is_shown && taken.insert(fold_upper(id)) {
                hidden.push(id.to_string());
            }
        }

        hidden
    }

    /// A Win32 menu string as a header. The shell marks the access key with `&` and the header
    /// with `_`, so each has to be translated and the other escaped; a tab introduces a shortcut
    /// hint that the app's menus never show.
    pub fn header(win32_text: Option<&str>) -> String {
        let Some(text) = win32_text.filter(|t| !t.is_empty()) else {
            return String::new();
        };
        let text = text.split('\t').next().unwrap_or("");

        let chars: Vec<char> = text.chars().collect();
        let mut result = String::with_capacity(text.len() + 4);
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '_' => result.push_str("__"),
                '&' if i + 1 < chars.len() && chars[i + 1] == '&' => {
                    result.push('&');
                    i += 1;
                }
                '&' => result.push('_'),
                c => result.push(c),
            }
            i += 1;
        }
        result
    }

    /// What a handler's menu looks like once the shell's habits are taken out: no separator at
    /// either end or beside another, no submenu with nothing in it, no item with no name — an
    /// owner-drawn item whose text the handler kept to itself has nothing to show.
    pub fn tidy<C, I>(entries: Vec<ShellMenuEntry<C, I>>) -> Vec<ShellMenuEntry<C, I>> {
        let mut kept: Vec<ShellMenuEntry<C, I>> = Vec::new();

        for mut entry in entries {
            if entry.is_separator {
                if kept.last().is_none_or(|last| last.is_separator) {
                    continue;
                }
                kept.push(entry);
                continue;
            }

            if entry.has_children() {
                let children = Self::tidy(std::mem::take(&mut entry.children));
                if children.is_empty() {
                    continue;
                }
                entry.children = children;
            } else if entry.command.is_none() {
                // A submenu header whose children were all dropped, or one the handler left empty.
                continue;
            }

            if entry.header.is_empty() {
                continue;
            }
            kept.push(entry);
        }

        while kept.last().is_some_and(|last| last.is_separator) {
            kept.pop();
        }
        kept
    }
}

fn is_offerable(verb: &ShellVerbRegistration) -> bool {
    if verb.extended || verb.legacy_disable || verb.programmatic_only {
        return false;
    }
    if verb.has_sub_commands || verb.has_delegate_execute {
        return false;
    }
    if verb.command.as_deref().is_none_or(|c| c.trim().is_empty()) {
        return false;
    }
    !ShellMenuRules::is_hidden_verb(&verb.verb)
}

fn capitalize(verb: &str) -> String {
    let mut chars = verb.chars();
    match chars.next() {
        Some(first) => {
            let mut out = String::with_capacity(verb.len());
            out.push(fold(first));
            out.push_str(chars.as_str());
            out
        }
        None => String::new(),
    }
}

/// The registration's key name, unless that is just the CLSID written out again, in which case
/// the class's own registered name is the better label.
fn handler_name(handler: &ShellHandlerRegistration) -> String {
    let name = handler.name.trim();
    if Guid::parse(name).is_none() && !name.is_empty() {
        return name.to_string();
    }
    match handler.class_name.as_deref().filter(|c| !c.is_empty()) {
        Some(class_name) => class_name.to_string(),
        None => handler.clsid.to_braced().to_uppercase(),
    }
}

fn append_family(families: &mut Vec<String>, family: &str) {
    if !families.iter().any(|f| eq_ignore_case(f, family)) {
        families.push(family.to_string());
    }
}
