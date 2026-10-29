//! New folder / new file in the folder a tab shows. Spec: ShellViewModel.CreateNewItemAsync,
//! SuggestNewItemName, PlanNewItem. Creating is additive, so there is no undo record; it shares the
//! transfer flag ("this app is writing"), selects what it made through the tab's pending selection,
//! and refreshes every tab showing the folder.

use std::sync::Arc;

use bb_core::newitem::{
    NewFileTemplate, NewItemExecutor, NewItemKind, NewItemPlan, NewItemPlanner,
};
use serde_json::{Value, json};

use crate::app::{Inner, lock};
use crate::ids::{AppError, AppResult, OpId, TabId};

impl Inner {
    /// The New items page: every type in menu order, the ones switched off included, and the state of
    /// "Import from Windows". `{types: [{label, extension, templatePath, enabled}], importing, status}`.
    pub(crate) fn new_types_state(&self) -> Value {
        let types: Vec<Value> = lock(&self.settings)
            .new_file_types
            .clone()
            .unwrap_or_else(NewFileTemplate::defaults)
            .iter()
            .map(|t| {
                json!({ "label": t.label, "extension": t.extension, "templatePath": t.template_path, "enabled": t.enabled })
            })
            .collect();
        let (importing, status) = lock(&self.extra.new_types_import).clone();
        json!({ "types": types, "importing": importing, "status": status })
    }

    /// Adds the types Windows offers that the list does not have yet, at its end; what is there keeps
    /// its place and its settings. Off the UI thread: the registry walk takes a while.
    /// Spec: SettingsViewModel.ImportFromWindowsAsync.
    pub(crate) fn new_types_import(self: &Arc<Self>) {
        {
            let mut state = lock(&self.extra.new_types_import);
            if state.0 {
                return;
            }
            *state = (true, String::new());
        }
        self.hub.emit(crate::events::AppEvent::SettingsChanged {
            key: "newFileTypes".into(),
        });
        self.schedule(|inner| {
            let discovered = inner.seams.shell_new.read();
            let existing = lock(&inner.settings)
                .new_file_types
                .clone()
                .unwrap_or_else(NewFileTemplate::defaults);
            let merged = bb_core::newitem::ShellNewImport::merge(&existing, &discovered);
            let added = merged.len() - existing.len();
            let mut status = if added == 0 {
                "Nothing new \u{2014} every type Windows offers is already listed.".to_string()
            } else {
                format!(
                    "Added {} type(s) from Windows.",
                    bb_core::format::thousands(added as i64)
                )
            };
            if added > 0 {
                let written = serde_json::to_string(&merged)
                    .map_err(|e| AppError::Invalid(e.to_string()))
                    .and_then(|text| inner.settings_set("newFileTypes", &text));
                if let Err(e) = written {
                    status = format!("Could not add them: {e}");
                }
            }
            *lock(&inner.extra.new_types_import) = (false, status);
            inner.hub.emit(crate::events::AppEvent::SettingsChanged {
                key: "newFileTypes".into(),
            });
        });
    }

    /// The file types the New submenu offers, in menu order.
    pub(crate) fn new_file_types(&self) -> Vec<NewFileTemplate> {
        lock(&self.settings)
            .new_file_types
            .clone()
            .unwrap_or_else(NewFileTemplate::defaults)
            .into_iter()
            .filter(|t| t.enabled)
            .collect()
    }

    /// The type a typed name belongs to: the one whose extension it ends with.
    fn template_for(&self, name: &str) -> Option<NewFileTemplate> {
        let lower = name.to_lowercase();
        self.new_file_types()
            .into_iter()
            .find(|t| !t.extension.is_empty() && lower.ends_with(&t.extension.to_lowercase()))
    }

    /// Where a new item goes: `directory` when given (a folder in the tree), else the folder the
    /// tab shows.
    fn new_item_directory(&self, tab: TabId, directory: Option<&str>) -> AppResult<String> {
        if let Some(directory) = directory {
            return Ok(directory.to_string());
        }
        let cell = self.tab(tab)?;
        let t = lock(&cell);
        if t.path.is_empty() || (t.is_flattened && !t.is_flat_browse) {
            return Err(AppError::Invalid(
                "There is no folder here to create anything in.".into(),
            ));
        }
        Ok(t.path.clone())
    }

    /// The name the New dialog opens with: the type's default, stepped aside to "(2)" if taken.
    pub(crate) fn new_item_suggest(
        &self,
        tab: TabId,
        directory: Option<&str>,
        kind: NewItemKind,
        extension: &str,
    ) -> AppResult<String> {
        let directory = self.new_item_directory(tab, directory)?;
        let planner = NewItemPlanner::local();
        Ok(match kind {
            NewItemKind::Folder => {
                planner.suggest_name(&directory, "New folder", NewItemKind::Folder, "")
            }
            NewItemKind::File => {
                let template = self
                    .new_file_types()
                    .into_iter()
                    .find(|t| t.extension.eq_ignore_ascii_case(extension));
                let base = template
                    .as_ref()
                    .map(NewFileTemplate::default_base_name)
                    .unwrap_or_else(|| "New file".to_string());
                planner.suggest_name(&directory, &base, NewItemKind::File, extension)
            }
        })
    }

    /// What creating `name` would produce, without changing anything: the dialog asks on every
    /// keystroke so it can refuse before anything is written.
    pub(crate) fn new_item_plan_json(
        &self,
        tab: TabId,
        directory: Option<&str>,
        name: &str,
        kind: NewItemKind,
    ) -> AppResult<Value> {
        let directory = self.new_item_directory(tab, directory)?;
        let plan = self.plan_new_item(&directory, name, kind);
        Ok(json!({
            "directory": plan.directory,
            "name": plan.name,
            "target": plan.target_path(),
            "ok": plan.has_work(),
            "rejected": plan.rejected.as_ref().map(|r| r.message.clone()),
        }))
    }

    fn plan_new_item(&self, directory: &str, name: &str, kind: NewItemKind) -> NewItemPlan {
        let template = (kind == NewItemKind::File)
            .then(|| self.template_for(name))
            .flatten();
        NewItemPlanner::local().plan(
            directory,
            name,
            kind,
            template.as_ref().and_then(|t| t.template_path.as_deref()),
        )
    }

    pub(crate) fn op_new_item(
        self: &Arc<Self>,
        tab: TabId,
        directory: Option<&str>,
        name: &str,
        kind: NewItemKind,
    ) -> AppResult<OpId> {
        let directory = self.new_item_directory(tab, directory)?;
        let plan = self.plan_new_item(&directory, name, kind);
        if let Some(rejected) = &plan.rejected {
            let text = format!("The create was refused: {}", rejected.message);
            self.set_status(tab, text.clone());
            return Err(AppError::Invalid(text));
        }
        self.submit_new_item(plan)
    }

    fn submit_new_item(self: &Arc<Self>, plan: NewItemPlan) -> AppResult<OpId> {
        self.submit(
            "New",
            false,
            Box::new(move |inner, _op, _cancel| {
                let mut outcome = NewItemExecutor.execute(&plan);
                let mut elevated = String::new();
                if let Some(retry) =
                    bb_core::elevation::ElevatedRetry::retry_for_new_item(&plan, &outcome)
                {
                    let item = format!("{}\\{}", plan.directory.trim_end_matches('\\'), plan.name);
                    let (second, note) = inner.elevate(
                        bb_core::elevation::ElevationOperation::NewItem,
                        &[item],
                        false,
                        |runner| runner.run_new_item(&retry, &bb_core::cancel::CancelToken::new()),
                    );
                    elevated = note;
                    if let Some(second) = second {
                        outcome = bb_core::elevation::ElevatedRetry::merge_new_item(
                            &outcome, &retry, &second,
                        );
                    }
                }
                if let Some(created) = &outcome.created_path {
                    inner.select_after_refresh(&plan.directory, created);
                }
                inner.refresh_tabs_showing(std::slice::from_ref(&plan.directory));
                let (ok, text) = match &outcome.failed {
                    Some(failed) => {
                        inner.seams.notice.say(&failed.message, "New");
                        (false, format!("{}{elevated}", failed.message))
                    }
                    None => (true, format!("Created '{}'", plan.name)),
                };
                inner.set_active_status(text.clone());
                (ok, text)
            }),
        )
    }
}
