//! The pane and tab arrangement as data: what closing the window saves, what a launch (or a named
//! workspace) rebuilds, and the pane and tab moves that reshape it. Spec: ShellViewModel (the
//! session half), SessionLayoutRules, PaneViewModel.MoveTab.

use std::sync::Arc;

use bb_core::columns::ColumnCatalog;
use bb_core::layout::{
    LayoutTree, NodeId, SessionLayout, SessionLayoutRules, SessionTab, SplitOrientation,
};

use crate::app::{Inner, Pane, Shell, TabData, lock};
use crate::events::AppEvent;
use crate::ids::{AppError, AppResult, PaneId, TabId};

fn capture_node(shell: &Shell, node: NodeId) -> SessionLayout {
    let weight = shell.layout.weight(node);
    if shell.layout.is_leaf(node) {
        let pane_id = *shell.layout.value(node);
        let mut tabs = Vec::new();
        let mut active = 0;
        if let Some(pane) = shell.panes.get(&pane_id) {
            for (i, id) in pane.tabs.iter().enumerate() {
                let Some(cell) = shell.tabs.get(id) else {
                    continue;
                };
                let t = lock(cell);
                if pane.active == Some(*id) {
                    active = i as i32;
                }
                tabs.push(SessionTab {
                    path: t.path.clone(),
                    sort_by: Some(t.sort_by.clone()),
                    sort_descending: t.sort_desc,
                    is_flat: t.is_flat_view,
                    columns: if t.columns_customized {
                        t.columns.clone()
                    } else {
                        None
                    },
                });
            }
        }
        return SessionLayout {
            orientation: None,
            weight,
            children: None,
            tabs: Some(tabs),
            active_tab_index: active,
            is_active_pane: shell.active_pane == pane_id,
        };
    }
    SessionLayout {
        orientation: shell.layout.orientation(node),
        weight,
        children: Some(
            shell
                .layout
                .children(node)
                .iter()
                .map(|c| capture_node(shell, *c))
                .collect(),
        ),
        tabs: None,
        active_tab_index: 0,
        is_active_pane: false,
    }
}

/// Builds `session` under the leaf `at`, recording each pane's saved content in `panes` in
/// on-screen order. New panes take ids from `next`.
fn build<'a>(
    tree: &mut LayoutTree<PaneId>,
    at: NodeId,
    session: &'a SessionLayout,
    next: &mut impl FnMut() -> PaneId,
    panes: &mut Vec<(PaneId, &'a SessionLayout)>,
) {
    let children = match (&session.children, session.orientation) {
        (Some(children), Some(orientation)) if !children.is_empty() => (children, orientation),
        _ => {
            panes.push((*tree.value(at), session));
            return;
        }
    };
    let (children, orientation) = children;
    let mut leaves = vec![at];
    for _ in 1..children.len() {
        let last = *leaves.last().expect("one");
        leaves.push(tree.split(last, orientation, next()));
    }
    let main = tree.parent_of(leaves[0]);
    for (leaf, child) in leaves.iter().zip(children) {
        build(tree, *leaf, child, next, panes);
    }
    if let Some(main) = main {
        for (leaf, child) in leaves.iter().zip(children) {
            let mut region = *leaf;
            while let Some(parent) = tree.parent_of(region) {
                if parent == main {
                    break;
                }
                region = parent;
            }
            tree.set_weight(
                region,
                if child.weight > 0.0 {
                    child.weight
                } else {
                    1.0
                },
            );
        }
    }
}

impl Inner {
    pub(crate) fn capture_layout(&self) -> SessionLayout {
        let shell = lock(&self.shell);
        capture_node(&shell, shell.layout.root())
    }

    /// Tears the arrangement down and rebuilds it from `layout`, pruned the way a launch prunes it.
    /// False when nothing in it is worth restoring (the arrangement is then left alone).
    pub(crate) fn apply_layout(self: &Arc<Self>, layout: SessionLayout) -> AppResult<bool> {
        let pruned = SessionLayoutRules::prune(Some(layout), &SessionLayoutRules::path_exists);
        if !SessionLayoutRules::is_usable(pruned.as_ref()) {
            return Ok(false);
        }
        let pruned = pruned.expect("usable");

        // Out with the old: every tab stops loading and watching.
        {
            let mut shell = lock(&self.shell);
            let old: Vec<_> = shell.tabs.drain().map(|(_, cell)| cell).collect();
            for cell in old {
                let mut t = lock(&cell);
                t.cancel.cancel();
                t.watcher = None;
            }
            shell.panes.clear();
        }

        let first = PaneId(self.new_id());
        let (mut tree, root) = LayoutTree::new(first);
        let mut panes = Vec::new();
        let mut next = || PaneId(self.new_id());
        build(&mut tree, root, &pruned, &mut next, &mut panes);
        {
            let mut shell = lock(&self.shell);
            shell.layout = tree;
            for (id, _) in &panes {
                shell.panes.insert(
                    *id,
                    Pane {
                        tabs: Vec::new(),
                        active: None,
                        closed_paths: Vec::new(),
                    },
                );
            }
            shell.active_pane = panes[0].0;
        }

        let mut active_pane = panes[0].0;
        for (pane, saved) in &panes {
            let tabs = saved.tabs.clone().unwrap_or_default();
            let mut ids = Vec::new();
            for saved_tab in &tabs {
                let tab = saved_tab.clone();
                let id = self.open_tab_with(*pane, &saved_tab.path.clone(), false, move |t| {
                    apply_saved_tab(t, &tab)
                })?;
                ids.push(id);
            }
            let index = (saved.active_tab_index.max(0) as usize).min(ids.len().saturating_sub(1));
            if let Some(active) = ids.get(index) {
                let mut shell = lock(&self.shell);
                if let Some(p) = shell.panes.get_mut(pane) {
                    p.active = Some(*active);
                }
            }
            if saved.is_active_pane {
                active_pane = *pane;
            }
        }
        lock(&self.shell).active_pane = active_pane;
        self.hub.emit(AppEvent::LayoutChanged);
        self.hub.emit(AppEvent::PaneChanged { pane: active_pane });
        Ok(true)
    }

    /// Writes the arrangement into the settings (the file is written by the caller's flush).
    pub(crate) fn session_save(&self) {
        let layout = self.capture_layout();
        // Separate statements: `self.tab` locks the shell, and the guard is not re-entrant.
        let active = lock(&self.shell).active_tab_id();
        // Never a hidden folder: next launch should not open somewhere the list would not show.
        // Spec: MainWindow.OnClosing (LastPath, IsHiddenDirectory).
        let last = active
            .and_then(|t| self.tab(t).ok())
            .map(|c| lock(&c).path.clone())
            .filter(|p| !p.is_empty() && !crate::commands::palette::is_hidden_dir(p));
        let mut s = lock(&self.settings);
        s.session = Some(layout);
        s.last_path = last;
    }

    /// The launch rule: the saved session when asked for and usable, otherwise one pane at the
    /// startup folder (or `fallback`). The first tab is returned.
    pub(crate) fn session_restore(self: &Arc<Self>, fallback: &str) -> AppResult<TabId> {
        let (restore, session, startup, last) = {
            let s = lock(&self.settings);
            (
                s.restore_last_session,
                s.session.clone(),
                s.startup_default_path.clone(),
                s.last_path.clone(),
            )
        };
        if restore
            && let Some(session) = session
            && self.apply_layout(session)?
        {
            return lock(&self.shell).active_tab_id().ok_or(AppError::Stale);
        }
        let start = if restore {
            last.filter(|p| std::path::Path::new(p).is_dir())
        } else {
            startup.filter(|p| std::path::Path::new(p).is_dir())
        }
        .unwrap_or_else(|| fallback.to_string());
        let pane = lock(&self.shell).active_pane;
        self.open_tab(pane, &start, true)
    }

    /// Capture, prune and rebuild in place: the harness's `session` verb.
    pub(crate) fn session_roundtrip(self: &Arc<Self>) -> AppResult<bool> {
        let layout = self.capture_layout();
        self.apply_layout(layout)
    }

    // ---- tab and pane moves ----

    /// Moves a tab to `index` within its pane (clamped).
    pub(crate) fn move_tab(&self, tab: TabId, index: usize) -> AppResult<()> {
        {
            let mut shell = lock(&self.shell);
            let pane = lock(shell.tabs.get(&tab).ok_or(AppError::Stale)?).pane;
            let p = shell.panes.get_mut(&pane).ok_or(AppError::Stale)?;
            let from = p
                .tabs
                .iter()
                .position(|t| *t == tab)
                .ok_or(AppError::Stale)?;
            let to = index.min(p.tabs.len() - 1);
            if from == to {
                return Ok(());
            }
            let id = p.tabs.remove(from);
            p.tabs.insert(to, id);
        }
        self.hub.emit(AppEvent::LayoutChanged);
        Ok(())
    }

    /// Moves a tab into another pane, activating it there. An emptied pane closes.
    pub(crate) fn move_tab_to_pane(&self, tab: TabId, target: PaneId) -> AppResult<()> {
        let emptied = {
            let mut shell = lock(&self.shell);
            let cell = shell.tabs.get(&tab).cloned().ok_or(AppError::Stale)?;
            let source = lock(&cell).pane;
            if source == target {
                return Ok(());
            }
            if !shell.panes.contains_key(&target) {
                return Err(AppError::Stale);
            }
            let src = shell.panes.get_mut(&source).ok_or(AppError::Stale)?;
            let index = src.tabs.iter().position(|t| *t == tab).unwrap_or(0);
            src.tabs.retain(|t| *t != tab);
            if src.active == Some(tab) {
                src.active = src
                    .tabs
                    .get(index.min(src.tabs.len().saturating_sub(1)))
                    .copied();
            }
            let empty = src.tabs.is_empty();
            lock(&cell).pane = target;
            let dst = shell.panes.get_mut(&target).ok_or(AppError::Stale)?;
            dst.tabs.push(tab);
            dst.active = Some(tab);
            shell.active_pane = target;
            empty.then_some(source)
        };
        if let Some(source) = emptied {
            self.close_pane(source)?;
        }
        self.hub.emit(AppEvent::LayoutChanged);
        Ok(())
    }

    /// Moves the active tab into a pane of its own beside its current one.
    pub(crate) fn move_tab_to_new_pane(self: &Arc<Self>, tab: TabId) -> AppResult<PaneId> {
        let (pane, path) = {
            let cell = self.tab(tab)?;
            let t = lock(&cell);
            (t.pane, t.path.clone())
        };
        let new_pane = PaneId(self.new_id());
        {
            let mut shell = lock(&self.shell);
            let node = shell.leaf(pane).ok_or(AppError::Stale)?;
            shell
                .layout
                .split(node, SplitOrientation::Vertical, new_pane);
            shell.panes.insert(
                new_pane,
                Pane {
                    tabs: Vec::new(),
                    active: None,
                    closed_paths: Vec::new(),
                },
            );
        }
        let _ = path;
        self.move_tab_to_pane(tab, new_pane)?;
        Ok(new_pane)
    }

    /// The pane that is not `pane`, next in on-screen order, when there is one.
    pub(crate) fn other_pane(&self, pane: PaneId) -> Option<PaneId> {
        let shell = lock(&self.shell);
        let leaves = shell.layout.leaves();
        if leaves.len() < 2 {
            return None;
        }
        let node = shell.leaf(pane)?;
        let next = shell.layout.next_leaf(node, 1);
        Some(*shell.layout.value(next))
    }

    pub(crate) fn swap_panes(&self) -> AppResult<()> {
        {
            let mut shell = lock(&self.shell);
            let active = shell.active_pane;
            let a = shell.leaf(active).ok_or(AppError::Stale)?;
            let b = shell.layout.next_leaf(a, 1);
            if a == b {
                return Ok(());
            }
            shell.layout.swap(a, b);
        }
        self.hub.emit(AppEvent::LayoutChanged);
        Ok(())
    }

    pub(crate) fn equalise_panes(&self) {
        lock(&self.shell).layout.equalise();
        self.hub.emit(AppEvent::LayoutChanged);
    }

    /// Closes every tab of a pane except `keep`.
    pub(crate) fn close_other_tabs(self: &Arc<Self>, keep: TabId) -> AppResult<()> {
        let keep_cell = self.tab(keep)?;
        let pane = lock(&keep_cell).pane;
        let others: Vec<TabId> = lock(&self.shell)
            .panes
            .get(&pane)
            .map(|p| p.tabs.iter().copied().filter(|t| *t != keep).collect())
            .unwrap_or_default();
        for t in others {
            self.close_tab(t)?;
        }
        Ok(())
    }
}

/// Puts a saved tab's arrangement on a fresh one before it navigates.
fn apply_saved_tab(t: &mut TabData, saved: &SessionTab) {
    if let Some(sort) = &saved.sort_by {
        t.sort_by = ColumnCatalog::sort_spec(Some(sort)).id;
    }
    t.sort_desc = saved.sort_descending;
    t.is_flat_view = saved.is_flat;
    // Customised when it was saved, and must stay that way: otherwise the next change to the
    // saved default would silently discard it.
    if let Some(columns) = &saved.columns {
        t.columns = Some(columns.clone());
        t.columns_customized = true;
    }
}
