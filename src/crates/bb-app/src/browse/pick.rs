//! A tab that is choosing a file for another program: it asked on the command line (`--pick`), is
//! waiting on a pipe it made, and gets one line back - the path - or nothing, which means the user
//! backed out. The tab is an ordinary tab in every other way: it navigates, searches and previews.
//!
//! What must stay true:
//! - **Only a path is ever returned.** Nothing is written, created or overwritten here; a save
//!   answers with where the file would go and the asker does the writing (and asks about replacing).
//! - **Nothing inside an archive is an answer**: it has no path another program can open.
//! - **Dropping the state is the cancel.** The reply channel closes when the last reference goes,
//!   so closing the tab, closing the window and Cancel are one thing, and none can be forgotten.
//! - **A pick tab is never saved in the session**: the question does not outlive the asker.

use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use bb_core::cli::{PickMode, PickRequest};
use serde_json::{Value, json};

use crate::app::{Inner, TabData, lock};
use crate::events::AppEvent;
use crate::ids::{AppError, AppResult, TabId};
use crate::seams::ReplyChannel;

type Channel = Arc<Mutex<Box<dyn ReplyChannel>>>;

/// How often a pick tab looks to see whether its asker is still waiting.
const WATCH_EVERY: Duration = Duration::from_millis(400);

/// What a tab holds while it is choosing a file for someone.
pub struct PickState {
    pub request: PickRequest,
    channel: Channel,
}

/// Whether the tab could answer now, and when it cannot, why, in words for a tooltip.
fn readiness(t: &TabData, request: &PickRequest) -> Result<(), String> {
    let in_archive = crate::commands::ArchivePathCheck::in_archive(&t.path);
    match request.mode {
        PickMode::Save => {
            if t.path.is_empty() || (t.is_flattened && !t.is_flat_browse) {
                Err("Go to the folder to save in.".to_string())
            } else if in_archive {
                Err("A file cannot be saved inside an archive.".to_string())
            } else {
                Ok(())
            }
        }
        PickMode::Open => {
            let mut selected = t
                .listing
                .rows()
                .iter()
                .filter(|r| t.selection.contains(&r.id));
            let (Some(row), None) = (selected.next(), selected.next()) else {
                return Err("Select one file.".to_string());
            };
            if row.is_directory {
                return Err("Select a file; a folder opens with Enter.".to_string());
            }
            if in_archive {
                return Err("Extract the file first: nothing inside an archive can be opened by another program.".to_string());
            }
            if !request.accepts(&row.name) {
                return Err(format!(
                    "Select a file of the kind asked for ({}).",
                    request.filter
                ));
            }
            Ok(())
        }
    }
}

fn heading(request: &PickRequest) -> String {
    if !request.title.trim().is_empty() {
        return request.title.clone();
    }
    match request.mode {
        PickMode::Open => "Choose a file".to_string(),
        PickMode::Save => "Choose where to save".to_string(),
    }
}

/// A typed name with the extension the asker's filter implies, when it has none of its own and the
/// filter's first pattern names one. `clip` under `*.mp4;*.gif` is `clip.mp4`; `clip.gif` is itself.
pub(crate) fn with_default_extension(name: &str, request: &PickRequest) -> String {
    if name.contains('.') {
        return name.to_string();
    }
    match request.patterns().first().and_then(|p| p.strip_prefix('*')) {
        Some(ext) if ext.starts_with('.') && !ext.contains(['*', '?']) => format!("{name}{ext}"),
        _ => name.to_string(),
    }
}

impl Inner {
    /// Takes a pick request: connects to the asker first, and only then opens a tab for it. A
    /// request nobody is waiting on opens nothing - a banner asking the user to choose a file for
    /// a program that is not there is worse than no banner.
    pub(crate) fn begin_pick(self: &Arc<Self>, request: &PickRequest) -> AppResult<()> {
        let channel = self
            .seams
            .replies
            .connect(&request.reply_pipe)
            .map_err(AppError::Invalid)?;
        let channel: Channel = Arc::new(Mutex::new(channel));

        // A folder to begin in, or a file to begin beside (selected, so a save beside the source
        // and an open of its neighbour both start in the right place).
        let (directory, selection) = if request.start.is_empty() {
            (None, None)
        } else {
            match crate::requests::resolve(&bb_core::cli::OpenTarget::new(&request.start, false)) {
                Some((directory, selection)) => (Some(directory), selection),
                None => (None, None),
            }
        };
        let pane = lock(&self.shell).active_pane;
        // Its own statement: `tab` takes the shell's lock too, and the guard of a temporary lives
        // to the end of the statement that made it.
        let active = lock(&self.shell).active_tab_id();
        let fallback = active
            .and_then(|t| self.tab(t).ok())
            .map(|cell| lock(&cell).path.clone())
            .unwrap_or_default();
        let directory = directory.unwrap_or(fallback);

        let state = PickState {
            request: request.clone(),
            channel: channel.clone(),
        };
        let tab = self.open_tab_with(pane, &directory, true, |t| {
            t.pick = Some(state);
            t.pending_selection = selection.into_iter().collect();
        })?;
        self.watch_pick(tab, &channel);
        Ok(())
    }

    /// Clears the tab's pick when the asker hangs up (it was closed, or its own Cancel was
    /// pressed), so the banner never outlives the question. The thread holds the channel weakly:
    /// it ends by itself once the tab has answered, cancelled or closed.
    fn watch_pick(self: &Arc<Self>, tab: TabId, channel: &Channel) {
        let app: Weak<Inner> = Arc::downgrade(self);
        let channel = Arc::downgrade(channel);
        let _ = std::thread::Builder::new()
            .name("bertbrowser pick watcher".to_string())
            .spawn(move || {
                loop {
                    std::thread::sleep(WATCH_EVERY);
                    let Some(channel) = channel.upgrade() else {
                        return;
                    };
                    let closed = lock(&channel).is_closed();
                    drop(channel);
                    if closed {
                        if let Some(app) = app.upgrade() {
                            app.end_pick(tab, "The program that asked stopped waiting.");
                        }
                        return;
                    }
                }
            });
    }

    /// The banner's model, or `null` for a tab that is not choosing anything:
    /// `{mode, title, filter, name, canAnswer, why}`.
    pub(crate) fn pick_json(&self, tab: TabId) -> Value {
        let Ok(cell) = self.tab(tab) else {
            return Value::Null;
        };
        let t = lock(&cell);
        let Some(pick) = &t.pick else {
            return Value::Null;
        };
        let ready = readiness(&t, &pick.request);
        json!({
            "mode": pick.request.mode.name(),
            "title": heading(&pick.request),
            "filter": pick.request.filter,
            "name": pick.request.default_name,
            "canAnswer": ready.is_ok(),
            "why": ready.err(),
        })
    }

    /// `choose` answers with the selected file, `save` with `name` in the folder being shown,
    /// `cancel` with nothing. `Err` carries words for the status line and leaves the question open.
    pub(crate) fn pick_do(self: &Arc<Self>, tab: TabId, verb: &str, name: &str) -> AppResult<()> {
        let cell = self.tab(tab)?;
        let answer = {
            let t = lock(&cell);
            let Some(pick) = &t.pick else {
                return Err(AppError::Invalid(
                    "This tab is not choosing a file.".to_string(),
                ));
            };
            match (verb, pick.request.mode) {
                ("cancel", _) => None,
                ("choose", PickMode::Open) => {
                    readiness(&t, &pick.request).map_err(AppError::Invalid)?;
                    t.listing
                        .rows()
                        .iter()
                        .find(|r| t.selection.contains(&r.id))
                        .map(|r| r.full_path().into_owned())
                }
                ("save", PickMode::Save) => {
                    readiness(&t, &pick.request).map_err(AppError::Invalid)?;
                    let name = with_default_extension(name.trim(), &pick.request);
                    if !PickRequest::is_acceptable_file_name(&name) {
                        return Err(AppError::Invalid(
                            "Type a file name: no \\ / : * ? \" < > | and not a path.".to_string(),
                        ));
                    }
                    Some(format!("{}\\{name}", t.path.trim_end_matches('\\')))
                }
                _ => {
                    return Err(AppError::Invalid(format!(
                        "'{verb}' is not something this tab can answer."
                    )));
                }
            }
        };
        self.answer_pick(tab, answer.as_deref())
    }

    /// Enter or a double-click on a file in a pick tab: the file is the answer. `None` when the
    /// tab is not choosing anything, and the row opens as it always does.
    pub(crate) fn pick_activate(
        self: &Arc<Self>,
        tab: TabId,
        row_path: &str,
        row_name: &str,
    ) -> Option<AppResult<()>> {
        let cell = self.tab(tab).ok()?;
        let refusal = {
            let t = lock(&cell);
            let pick = t.pick.as_ref()?;
            if crate::commands::ArchivePathCheck::in_archive(&t.path) {
                Some(format!("Extract {row_name} first to choose it."))
            } else if !pick.request.accepts(row_name) {
                // An archive that is not what was asked for is still a folder to look in.
                if bb_core::archives::ArchiveFormats::is_archive_name(row_name) {
                    return None;
                }
                Some(format!(
                    "{row_name} is not the kind of file asked for ({}).",
                    pick.request.filter
                ))
            } else {
                None
            }
        };
        Some(match refusal {
            // Never launched: the user is answering a question, not opening a file.
            Some(why) => {
                self.set_status(tab, why);
                Ok(())
            }
            None => self.answer_pick(tab, Some(row_path)),
        })
    }

    /// Sends the answer (or, for `None`, hangs up without one) and ends the pick.
    fn answer_pick(self: &Arc<Self>, tab: TabId, answer: Option<&str>) -> AppResult<()> {
        let cell = self.tab(tab)?;
        let Some(pick) = lock(&cell).pick.take() else {
            return Ok(());
        };
        let sent = match answer {
            Some(path) => lock(&pick.channel).send_line(path),
            None => Ok(()),
        };
        // Dropped here either way, which closes the pipe: after an answer that is the end of the
        // line, and with none it is the cancel.
        drop(pick);
        match (&sent, answer) {
            (Err(why), _) => self.set_status(tab, format!("The answer was not delivered: {why}")),
            (Ok(()), Some(path)) => self.set_status(tab, format!("Chose {path}")),
            (Ok(()), None) => {}
        }
        self.finish_pick(tab);
        sent.map_err(AppError::Invalid)
    }

    /// The asker went away: the question is withdrawn and the tab stays, as an ordinary tab.
    fn end_pick(self: &Arc<Self>, tab: TabId, status: &str) {
        let Ok(cell) = self.tab(tab) else { return };
        if lock(&cell).pick.take().is_none() {
            return;
        }
        self.set_status(tab, status.to_string());
        self.hub.emit(AppEvent::TabChanged { tab });
        self.hub.emit(AppEvent::LayoutChanged);
    }

    /// The tab was opened for the question, so it goes with it - unless it is the pane's only tab,
    /// which closing would take the pane (or the window's last pane) with it.
    fn finish_pick(self: &Arc<Self>, tab: TabId) {
        let alone = {
            let shell = lock(&self.shell);
            shell
                .tabs
                .get(&tab)
                .map(|cell| lock(cell).pane)
                .and_then(|pane| shell.panes.get(&pane))
                .is_none_or(|p| p.tabs.len() <= 1)
        };
        if alone || self.close_tab(tab).is_err() {
            self.hub.emit(AppEvent::TabChanged { tab });
            self.hub.emit(AppEvent::LayoutChanged);
        }
    }
}
