//! Opening what the command line (or a second launch's hand-off) asked for.
//! Spec: ShellViewModel.OpenRequestAsync / Resolve, App.RemainingAfterStart.

use std::path::Path;
use std::sync::Arc;

use bb_core::archives::ArchivePath;
use bb_core::cli::{CommandLineRequest, OpenIn, OpenTarget};
use bb_core::layout::SplitOrientation;

use crate::app::{Inner, lock};
use crate::ids::AppResult;

/// What a target comes to: the folder to show and what to highlight in it. None when it names
/// nothing that exists.
pub(crate) fn resolve(target: &OpenTarget) -> Option<(String, Option<String>)> {
    let path = target.path.as_str();
    if Path::new(path).is_dir() {
        // A folder asked for with /select is highlighted in its parent, like Explorer.
        if !target.select {
            return Some((path.to_string(), None));
        }
        return Some(
            match bb_core::rename::paths::directory_name(path.trim_end_matches('\\')) {
                Some(above) if !above.is_empty() => (above, Some(path.to_string())),
                _ => (path.to_string(), None),
            },
        );
    }
    // A bare archive keeps reveal-and-select: "bertbrowser C:\x\a.zip" typed at a prompt most
    // plausibly means "show me that file", and the shell folder-handler registration only ever
    // sends directories, so nothing is lost by not entering it here.
    if Path::new(path).is_file() {
        return bb_core::rename::paths::directory_name(path).map(|d| (d, Some(path.to_string())));
    }
    // A path *inside* an archive has no other reading.
    if ArchivePath::parse(path, |p| Path::new(p).is_file()).is_some() {
        return Some((path.to_string(), None));
    }
    None
}

/// What is left to open once the first folder target has become the start path: everything that
/// asked to be selected, and every folder that is not the one already shown.
pub fn remaining_after_start(
    request: &CommandLineRequest,
    start_path: Option<&str>,
) -> CommandLineRequest {
    CommandLineRequest {
        targets: request
            .targets
            .iter()
            .filter(|t| t.select || !start_path.is_some_and(|s| s.eq_ignore_ascii_case(&t.path)))
            .cloned()
            .collect(),
        mode: request.mode,
        errors: request.errors.clone(),
    }
}

impl Inner {
    pub(crate) fn open_request(self: &Arc<Self>, request: &CommandLineRequest) -> AppResult<()> {
        for target in &request.targets {
            let Some((directory, selection)) = resolve(target) else {
                continue;
            };
            let pending = selection.into_iter().collect::<Vec<_>>();
            let pane = lock(&self.shell).active_pane;
            match request.mode {
                OpenIn::NewTab => {
                    self.open_tab_with(pane, &directory, true, |t| t.pending_selection = pending)?;
                }
                OpenIn::NewPane => {
                    let new_pane = self.split_pane(pane, SplitOrientation::Vertical, &directory)?;
                    let tab = lock(&self.shell)
                        .panes
                        .get(&new_pane)
                        .and_then(|p| p.active);
                    if let Some(tab) = tab
                        && !pending.is_empty()
                    {
                        let cell = self.tab(tab)?;
                        lock(&cell).pending_selection = pending;
                        self.refresh_view(tab);
                    }
                }
                OpenIn::Default => {
                    let Some(tab) = lock(&self.shell).active_tab_id() else {
                        self.open_tab_with(pane, &directory, true, |t| {
                            t.pending_selection = pending
                        })?;
                        continue;
                    };
                    // Set before navigating, not after: the listing that arrives is the one the
                    // selection belongs to, and when the tab is *already* showing this folder there
                    // is no reload at all, and setting it is the only signal there will be.
                    let cell = self.tab(tab)?;
                    lock(&cell).pending_selection = pending;
                    let current = lock(&cell).path.clone();
                    if current.eq_ignore_ascii_case(&directory) {
                        self.refresh_view(tab);
                    } else {
                        self.navigate(tab, &directory)?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(path: &str, select: bool) -> OpenTarget {
        OpenTarget::new(path, select)
    }

    #[test]
    fn a_folder_is_shown_and_a_folder_with_select_is_highlighted_in_its_parent() {
        let windows = r"C:\Windows";
        assert_eq!(
            Some((windows.to_string(), None)),
            resolve(&target(windows, false))
        );
        assert_eq!(
            Some((r"C:\".to_string(), Some(windows.to_string()))),
            resolve(&target(windows, true))
        );
    }

    #[test]
    fn a_file_is_revealed_in_its_folder() {
        let file = r"C:\Windows\explorer.exe";
        assert_eq!(
            Some((r"C:\Windows".to_string(), Some(file.to_string()))),
            resolve(&target(file, false))
        );
    }

    #[test]
    fn something_that_does_not_exist_is_dropped_not_guessed_at() {
        assert_eq!(None, resolve(&target(r"C:\No\Such\Folder\Anywhere", false)));
    }

    #[test]
    fn the_start_folder_is_not_opened_twice_but_a_select_always_is() {
        let request = CommandLineRequest {
            targets: vec![
                target(r"C:\a", false),
                target(r"C:\b", false),
                target(r"C:\a", true),
            ],
            mode: OpenIn::NewTab,
            errors: Vec::new(),
        };
        let rest = remaining_after_start(&request, Some(r"c:\A"));
        assert_eq!(
            vec![target(r"C:\b", false), target(r"C:\a", true)],
            rest.targets
        );
        assert_eq!(OpenIn::NewTab, rest.mode);
    }
}
