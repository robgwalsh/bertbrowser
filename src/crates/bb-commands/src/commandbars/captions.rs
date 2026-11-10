//! What a command is called on a menu, where that differs from its catalogue name. The catalogue
//! names a command for the palette, which wants a verb to type ("Toggle preview pane"); a menu
//! shows a tick, so it wants the thing ticked ("Preview pane").
//! Spec: Core/Services/CommandBars/CommandBarCaptions.cs.

use super::{AccessKeys, BarItem, BarItemKind, CommandBarLists};
use crate::commands::CommandCatalog;

const SHORT: &[(&str, &str)] = &[
    ("nav.up", "Up"),
    ("view.preview", "Preview pane"),
    ("view.metadata", "Metadata pane"),
    ("view.flat", "Flat view"),
    ("view.hidden", "Hidden items"),
    ("preview.wrap", "Word wrap"),
    ("preview.fit", "Fit image to pane"),
    ("preview.auto-advance", "Auto-advance"),
    ("preview.loop", "Loop"),
    ("preview.autoplay", "Autoplay"),
    ("tools.compare-differences", "Show only differences"),
    ("tools.compare-rescan", "Rescan comparison"),
    ("tools.compare-sync", "Sync\u{2026}"),
    ("app.match-windows-theme", "Match Windows light/dark"),
    ("app.command-bars", "Customise menus and toolbars\u{2026}"),
    ("app.about", "About BertBrowser"),
];

pub struct CommandBarCaptions;

impl CommandBarCaptions {
    /// The menu's words for a command, or None when it has none of its own and the command's name
    /// stands.
    pub fn for_id(id: &str) -> Option<&'static str> {
        SHORT
            .iter()
            .find(|(key, _)| *key == id)
            .map(|(_, text)| *text)
    }

    /// Every id given a caption here, for the test that none has gone stale.
    pub fn ids() -> impl Iterator<Item = &'static str> {
        SHORT.iter().map(|(id, _)| *id)
    }

    /// What an item reads as with nothing but the model to go on — the editor's row, and the
    /// fallback when a command's live state has no caption of its own.
    pub fn describe(item: &BarItem) -> String {
        match &item.kind {
            BarItemKind::Separator => "Separator".to_string(),
            BarItemKind::Menu => AccessKeys::strip(item.caption.as_deref().unwrap_or("")),
            BarItemKind::List => item
                .id
                .as_deref()
                .and_then(CommandBarLists::find)
                .map(|l| l.name.to_string())
                .or_else(|| item.id.clone())
                .unwrap_or_default(),
            _ => {
                if let Some(caption) = item.caption.as_deref().filter(|c| !c.is_empty()) {
                    AccessKeys::strip(caption)
                } else if let Some(id) = &item.id {
                    Self::for_id(id)
                        .map(str::to_string)
                        .or_else(|| CommandCatalog::find(id).map(|c| c.name.to_string()))
                        .unwrap_or_else(|| id.clone())
                } else {
                    String::new()
                }
            }
        }
    }
}
