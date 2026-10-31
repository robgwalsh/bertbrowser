//! Saved searches and saved workspaces: the named things the switchers, the palette and the two
//! Settings pages list. A search is "navigate, then type" (`SavedSearchRules::plan`); a workspace is
//! a captured arrangement of panes and tabs rebuilt in place. Where either is *offered* (sidebar,
//! title bar, hidden) is a setting; Settings lists them whichever is chosen.
//! Spec: SavedSearchesViewModel, SavedWorkspacesViewModel, ShellViewModel.RunSavedSearchAsync,
//! SwitchWorkspaceAsync.

use bb_core::FileTime;
use bb_core::archives::ArchivePath;
use bb_core::data::{
    SavedSearch, SavedSearchRepository, SavedSearchScope, SavedWorkspace, SavedWorkspaceRepository,
};
use bb_core::format;
use bb_core::rules::{SavedSearchRules, SavedWorkspaceRules};
use serde_json::{Value, json};

use crate::app::{Inner, lock};
use crate::events::Model;
use crate::ids::{AppError, AppResult, TabId};

fn scope_name(scope: SavedSearchScope) -> &'static str {
    match scope {
        SavedSearchScope::CurrentFolder => "current",
        SavedSearchScope::Folder => "folder",
        SavedSearchScope::ThisPc => "pc",
    }
}

/// What a saved search's menu row says beside its name. Spec: SavedSearchViewModel.ScopeColumnText.
pub(crate) fn scope_column_text(scope: SavedSearchScope) -> &'static str {
    match scope {
        SavedSearchScope::CurrentFolder => "Current folder",
        SavedSearchScope::Folder => "Pinned folder",
        SavedSearchScope::ThisPc => "This PC",
    }
}

fn scope_text(search: &SavedSearch) -> String {
    match search.scope {
        SavedSearchScope::CurrentFolder => "Current folder".to_string(),
        SavedSearchScope::Folder => search.scope_path.clone().unwrap_or_default(),
        SavedSearchScope::ThisPc => "This PC".to_string(),
    }
}

impl Inner {
    fn searches_repo(&self) -> SavedSearchRepository {
        SavedSearchRepository::new(self.db.clone())
    }

    fn workspaces_repo(&self) -> SavedWorkspaceRepository {
        SavedWorkspaceRepository::new(self.db.clone())
    }

    fn db_err(e: impl std::fmt::Display) -> AppError {
        AppError::Invalid(e.to_string())
    }

    pub(crate) fn saved_searches(&self) -> Vec<SavedSearch> {
        self.searches_repo().get_all().unwrap_or_default()
    }

    pub(crate) fn saved_workspaces(&self) -> Vec<SavedWorkspace> {
        self.workspaces_repo().get_all().unwrap_or_default()
    }

    fn saved_changed(&self) {
        self.hub.model(Model::Saved);
    }

    // ---- searches ----

    /// The first reason this search cannot be saved, or None.
    pub(crate) fn search_problem(
        &self,
        name: &str,
        query: &str,
        scope: SavedSearchScope,
        scope_path: Option<&str>,
        replacing: Option<&str>,
    ) -> Option<String> {
        let repo = self.searches_repo();
        SavedSearchRules::validate(
            name,
            query,
            scope,
            scope_path,
            |n| {
                !replacing.is_some_and(|r| r.eq_ignore_ascii_case(n))
                    && repo.exists(n).unwrap_or(false)
            },
            |p| std::path::Path::new(p).is_file(),
        )
    }

    pub(crate) fn search_name_taken(&self, name: &str) -> bool {
        self.searches_repo().exists(name).unwrap_or(false)
    }

    /// Saves (or replaces, by name) a search. `scope_path` is the pinned folder for a `Folder` scope.
    pub(crate) fn search_save(
        &self,
        name: &str,
        query: &str,
        scope: SavedSearchScope,
        scope_path: Option<&str>,
    ) -> AppResult<()> {
        // Saving over an existing name replaces it; the dialog asks first, the model does not.
        if let Some(problem) = self.search_problem(name, query, scope, scope_path, Some(name)) {
            return Err(AppError::Invalid(problem));
        }
        self.searches_repo()
            .save(&SavedSearch::new(name.trim(), query, scope, scope_path))
            .map_err(Self::db_err)?;
        self.saved_changed();
        Ok(())
    }

    /// Saves what the tab is searching for: `scope` is `current`, `folder` (pinned to where the tab
    /// is) or `pc`.
    pub(crate) fn search_save_current(&self, tab: TabId, name: &str, scope: &str) -> AppResult<()> {
        let cell = self.tab(tab)?;
        let (query, path, global) = {
            let t = lock(&cell);
            (
                t.active_search_text().to_string(),
                t.path.clone(),
                t.is_global_search(),
            )
        };
        let (scope, pinned) = match scope {
            "folder" => (SavedSearchScope::Folder, Some(path)),
            "pc" => (SavedSearchScope::ThisPc, None),
            "" | "current" => (
                if global {
                    SavedSearchScope::ThisPc
                } else {
                    SavedSearchScope::CurrentFolder
                },
                None,
            ),
            other => return Err(AppError::Invalid(format!("unknown scope '{other}'"))),
        };
        self.search_save(name, &query, scope, pinned.as_deref())
    }

    /// The name a new saved search starts with.
    pub(crate) fn search_default_name(&self, tab: TabId) -> AppResult<String> {
        let cell = self.tab(tab)?;
        let text = lock(&cell).active_search_text().to_string();
        Ok(SavedSearchRules::default_name(&text))
    }

    pub(crate) fn search_run_saved(
        self: &std::sync::Arc<Self>,
        name: &str,
        new_tab: bool,
    ) -> AppResult<()> {
        let search = self
            .saved_searches()
            .into_iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| AppError::Invalid(format!("no saved search '{name}'")))?;
        let (tab, pane) = {
            let shell = lock(&self.shell);
            (
                shell.active_tab_id().ok_or(AppError::Stale)?,
                shell.active_pane,
            )
        };
        let tab_cell = self.tab(tab)?;
        let current = lock(&tab_cell).path.clone();
        let run = SavedSearchRules::plan(&search, &current)
            .ok_or_else(|| AppError::Invalid("There is nowhere to run this search from.".into()))?;
        // Always "navigate, then type", even when the folder is the one already showing.
        let target = if new_tab {
            self.open_tab(pane, &run.navigate_to, true)?
        } else {
            self.navigate(tab, &run.navigate_to)?;
            tab
        };
        self.set_search(target, &run.text, run.global)?;
        let _ = self
            .searches_repo()
            .mark_used(&search.name, FileTime::now());
        self.saved_changed();
        Ok(())
    }

    pub(crate) fn search_rename(&self, old: &str, new: &str) -> AppResult<()> {
        let new = new.trim();
        if new.is_empty() {
            return Err(AppError::Invalid("Give the search a name.".into()));
        }
        let renamed = self
            .searches_repo()
            .rename(old, new)
            .map_err(Self::db_err)?;
        if !renamed {
            return Err(AppError::Invalid(format!(
                "There is already a saved search called \"{new}\", or none called \"{old}\"."
            )));
        }
        self.saved_changed();
        Ok(())
    }

    pub(crate) fn search_remove(&self, name: &str) -> AppResult<()> {
        self.searches_repo().remove(name).map_err(Self::db_err)?;
        self.saved_changed();
        Ok(())
    }

    // ---- workspaces ----

    pub(crate) fn workspace_problem(&self, name: &str) -> Option<String> {
        let repo = self.workspaces_repo();
        SavedWorkspaceRules::validate(name, |n| repo.exists(n).unwrap_or(false))
    }

    /// Whether a workspace of this name is saved already (saving it would replace that one).
    pub(crate) fn workspace_name_taken(&self, name: &str) -> bool {
        self.workspaces_repo().exists(name).unwrap_or(false)
    }

    /// Captures the arrangement under a name (replacing one of that name).
    pub(crate) fn workspace_save(&self, name: &str) -> AppResult<()> {
        let repo = self.workspaces_repo();
        if let Some(problem) = SavedWorkspaceRules::validate(name, |_| false) {
            return Err(AppError::Invalid(problem));
        }
        let layout = self.capture_layout();
        repo.save(&SavedWorkspace::new(name.trim(), layout))
            .map_err(Self::db_err)?;
        *lock(&self.extra.current_workspace) = Some(name.trim().to_string());
        self.saved_changed();
        Ok(())
    }

    pub(crate) fn workspace_default_name(&self) -> String {
        SavedWorkspaceRules::default_name(FileTime::now().to_local())
    }

    pub(crate) fn workspace_switch(self: &std::sync::Arc<Self>, name: &str) -> AppResult<()> {
        let workspace = self
            .saved_workspaces()
            .into_iter()
            .find(|w| w.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| AppError::Invalid(format!("no saved workspace '{name}'")))?;
        if !self.apply_layout(workspace.layout.clone())? {
            let text = format!("Nothing in \"{}\" still exists.", workspace.name);
            self.set_active_status(text.clone());
            return Err(AppError::Invalid(text));
        }
        let _ = self
            .workspaces_repo()
            .mark_used(&workspace.name, FileTime::now());
        *lock(&self.extra.current_workspace) = Some(workspace.name.clone());
        self.saved_changed();
        // Once the new tabs have listed, or their item counts would say over it.
        let me = self.clone();
        let text = format!("Switched to workspace \"{}\"", workspace.name);
        std::thread::spawn(move || me.set_final_status(text));
        Ok(())
    }

    pub(crate) fn workspace_rename_problem(&self, old: &str, new: &str) -> Option<String> {
        SavedWorkspaceRules::validate(new, |n| {
            !n.eq_ignore_ascii_case(old) && self.workspaces_repo().exists(n).unwrap_or(false)
        })
    }

    pub(crate) fn workspace_rename(&self, old: &str, new: &str) -> AppResult<()> {
        if let Some(problem) = SavedWorkspaceRules::validate(new, |n| {
            !n.eq_ignore_ascii_case(old) && self.workspaces_repo().exists(n).unwrap_or(false)
        }) {
            return Err(AppError::Invalid(problem));
        }
        let renamed = self
            .workspaces_repo()
            .rename(old, new.trim())
            .map_err(Self::db_err)?;
        if !renamed {
            return Err(AppError::Invalid(format!("no saved workspace '{old}'")));
        }
        {
            let mut current = lock(&self.extra.current_workspace);
            if current
                .as_deref()
                .is_some_and(|c| c.eq_ignore_ascii_case(old))
            {
                *current = Some(new.trim().to_string());
            }
        }
        self.saved_changed();
        Ok(())
    }

    pub(crate) fn workspace_remove(&self, name: &str) -> AppResult<()> {
        self.workspaces_repo().remove(name).map_err(Self::db_err)?;
        {
            let mut current = lock(&self.extra.current_workspace);
            if current
                .as_deref()
                .is_some_and(|c| c.eq_ignore_ascii_case(name))
            {
                *current = None;
            }
        }
        self.saved_changed();
        Ok(())
    }

    pub(crate) fn saved_json(&self) -> Value {
        let settings = lock(&self.settings);
        let placement = |p: bb_core::settings::SectionPlacement| match p {
            bb_core::settings::SectionPlacement::Sidebar => "sidebar",
            bb_core::settings::SectionPlacement::TitleBar => "titlebar",
            bb_core::settings::SectionPlacement::Hidden => "hidden",
        };
        let (wp, sp) = (
            placement(settings.workspaces_placement),
            placement(settings.saved_searches_placement),
        );
        drop(settings);
        // The lists read as days ("Today 14:05"), in local time. Spec: SavedSearchViewModel.CreatedText.
        let now = FileTime::now().to_local();
        let created = |t: Option<FileTime>| format::day(t.map(FileTime::to_local), now, "Unknown");
        let used = |t: Option<FileTime>| format::day(t.map(FileTime::to_local), now, "Never");
        json!({
            "searches": self.saved_searches().iter().map(|s| json!({
                "name": s.name,
                "query": s.query,
                "scope": scope_name(s.scope),
                "scopePath": s.scope_path,
                "scopeText": scope_text(s),
                "scopeColumn": scope_column_text(s.scope),
                "pinnedInArchive": s.scope_path.as_deref().is_some_and(ArchivePath::looks_virtual),
                "createdUtc": s.created_utc.map(|t| t.0),
                "lastUsedUtc": s.last_used_utc.map(|t| t.0),
                "createdText": created(s.created_utc),
                "lastUsedText": used(s.last_used_utc),
            })).collect::<Vec<_>>(),
            "workspaces": self.saved_workspaces().iter().map(|w| json!({
                "name": w.name,
                "shape": SavedWorkspaceRules::shape_text(&w.layout),
                "folders": SavedWorkspaceRules::folders(&w.layout),
                "createdUtc": w.created_utc.map(|t| t.0),
                "lastUsedUtc": w.last_used_utc.map(|t| t.0),
                "createdText": created(w.created_utc),
                "lastUsedText": used(w.last_used_utc),
            })).collect::<Vec<_>>(),
            "bookmarks": self.bookmark_rows(),
            "workspacesPlacement": wp,
            "savedSearchesPlacement": sp,
            "currentWorkspace": *lock(&self.extra.current_workspace),
        })
    }
}
