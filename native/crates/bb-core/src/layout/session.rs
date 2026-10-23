//! A saved pane arrangement, in a shape that survives being written to settings.json and read back.
//! The mirror of the live tree rather than the thing itself: it carries only where the splits are,
//! how the space was divided, and what each pane had open. A node is a split or a pane, never both,
//! which is why `children` and `tabs` are both optional; hand-edited or half-written JSON has to be
//! survivable, so `SessionLayoutRules` decides what is a layout rather than the deserialiser failing.
//! Spec: Core/Layout/SessionLayout.cs.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::SplitOrientation;
use crate::archives::ArchivePath;
use crate::columns::ColumnSetting;

fn one() -> f64 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SessionLayout {
    /// None for a pane; set for a split.
    pub orientation: Option<SplitOrientation>,
    /// This node's share of its parent, as a star weight.
    #[serde(default = "one")]
    pub weight: f64,
    /// A split's children, in on-screen order. None for a pane.
    pub children: Option<Vec<SessionLayout>>,
    /// A pane's open directories, in tab order. None for a split.
    pub tabs: Option<Vec<SessionTab>>,
    /// Which of `tabs` was showing.
    pub active_tab_index: i32,
    /// True when this node was the pane the window chrome was following.
    pub is_active_pane: bool,
}

impl Default for SessionLayout {
    fn default() -> Self {
        SessionLayout {
            orientation: None,
            weight: 1.0,
            children: None,
            tabs: None,
            active_tab_index: 0,
            is_active_pane: false,
        }
    }
}

impl SessionLayout {
    pub fn pane(paths: &[&str]) -> SessionLayout {
        SessionLayout {
            tabs: Some(paths.iter().map(|p| SessionTab::at(p)).collect()),
            ..Default::default()
        }
    }

    pub fn split(orientation: SplitOrientation, children: Vec<SessionLayout>) -> SessionLayout {
        SessionLayout {
            orientation: Some(orientation),
            children: Some(children),
            ..Default::default()
        }
    }

    pub fn is_split(&self) -> bool {
        self.children.as_ref().is_some_and(|c| !c.is_empty())
    }
}

/// One open directory, and how it was being shown.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct SessionTab {
    pub path: String,
    /// The sort column's id. A string so an unknown value from a newer or hand-edited file falls
    /// back instead of failing the whole layout.
    pub sort_by: Option<String>,
    pub sort_descending: bool,
    /// Whether this tab was showing a flat branch view: part of how this tab was arranged rather
    /// than a preference every tab shares.
    pub is_flat: bool,
    /// The columns this tab was showing, when arranged here rather than taken from the saved
    /// default. **None means "whatever the default is"**.
    pub columns: Option<Vec<ColumnSetting>>,
}

impl SessionTab {
    pub fn at(path: &str) -> SessionTab {
        SessionTab {
            path: path.to_string(),
            ..Default::default()
        }
    }
}

/// Turning a saved layout back into something safe to rebuild from. Everything here is defensive: a
/// layout that cannot be honoured must degrade to the ordinary single-pane start rather than fail.
pub struct SessionLayoutRules;

impl SessionLayoutRules {
    /// More panes than anyone arranges deliberately; a guard against a file that claims thousands.
    pub const MAX_PANES: usize = 32;
    /// Tabs per pane, same reasoning.
    pub const MAX_TABS_PER_PANE: usize = 64;

    /// Drops what no longer exists and returns a layout worth rebuilding, or None when nothing is
    /// left. A pane whose every tab has gone is dropped, and a split left with one child collapses
    /// into it, the same rules the live tree enforces.
    pub fn prune(
        node: Option<SessionLayout>,
        exists: &impl Fn(&str) -> bool,
    ) -> Option<SessionLayout> {
        let mut node = node?;
        if !node.is_split() {
            let tabs: Vec<SessionTab> = node
                .tabs
                .take()
                .unwrap_or_default()
                .into_iter()
                .filter(|t| !t.path.is_empty() && exists(&t.path))
                .take(Self::MAX_TABS_PER_PANE)
                .collect();
            if tabs.is_empty() {
                return None;
            }
            node.active_tab_index = node.active_tab_index.clamp(0, tabs.len() as i32 - 1);
            node.tabs = Some(tabs);
            node.children = None;
            node.weight = sane_weight(node.weight);
            return Some(node);
        }

        let kept: Vec<SessionLayout> = node
            .children
            .take()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|child| Self::prune(Some(child), exists))
            .collect();
        if kept.is_empty() {
            return None;
        }
        // A split of one is not a split. Hoisting rather than keeping an empty level is what stops a
        // restored layout carrying splitters with nothing on one side.
        if kept.len() == 1 {
            let mut only = kept.into_iter().next().expect("one");
            only.weight = sane_weight(node.weight);
            return Some(only);
        }
        node.children = Some(kept);
        node.tabs = None;
        node.weight = sane_weight(node.weight);
        Some(node)
    }

    /// Whether a pruned layout is worth restoring at all.
    pub fn is_usable(node: Option<&SessionLayout>) -> bool {
        node.is_some_and(|n| {
            let count = Self::count_panes(n);
            count > 0 && count <= Self::MAX_PANES
        })
    }

    pub fn count_panes(node: &SessionLayout) -> usize {
        if !node.is_split() {
            return 1;
        }
        node.children
            .as_ref()
            .map_or(0, |c| c.iter().map(Self::count_panes).sum())
    }

    /// Every pane, in on-screen order.
    pub fn panes(node: &SessionLayout) -> Vec<&SessionLayout> {
        let mut out = Vec::new();
        collect_panes(node, &mut out);
        out
    }

    /// Whether a saved tab's path is still somewhere worth reopening: a real directory, or somewhere
    /// inside an archive. The one definition restoring at launch and restoring a named workspace share.
    pub fn path_exists(path: &str) -> bool {
        Path::new(path).is_dir() || ArchivePath::parse(path, |p| Path::new(p).is_file()).is_some()
    }
}

fn collect_panes<'a>(node: &'a SessionLayout, out: &mut Vec<&'a SessionLayout>) {
    if !node.is_split() {
        out.push(node);
        return;
    }
    if let Some(children) = &node.children {
        for child in children {
            collect_panes(child, out);
        }
    }
}

/// A weight of zero, NaN or infinity would give a grid a column it can never lay out.
fn sane_weight(weight: f64) -> f64 {
    if weight.is_finite() && weight > 0.0 {
        weight
    } else {
        1.0
    }
}
