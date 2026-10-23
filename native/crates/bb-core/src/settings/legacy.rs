//! The native app's first launch over a data directory the C# app used. Its database, settings
//! file, themes and templates are not read — the schema and the settings shape are both fresh —
//! but they are the user's, so they are **moved aside, never deleted**, into a `legacy-<yyyymmdd>`
//! folder beside them. It happens once: a marker file written on that first launch is what makes
//! every later launch leave the (now native) files alone. A pure plan over a directory listing,
//! and an executor that re-checks the disk before each move; one item's failure never affects the
//! others.

use std::path::Path;

use crate::FileTime;

/// One entry of the data directory, as the plan sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataDirEntry {
    pub name: String,
    pub is_directory: bool,
}

impl DataDirEntry {
    pub fn file(name: &str) -> DataDirEntry {
        DataDirEntry {
            name: name.to_string(),
            is_directory: false,
        }
    }

    pub fn dir(name: &str) -> DataDirEntry {
        DataDirEntry {
            name: name.to_string(),
            is_directory: true,
        }
    }
}

/// One move, both ends relative to the data directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyMove {
    pub from: String,
    pub to: String,
}

impl LegacyMove {
    pub fn new(from: &str, to: &str) -> LegacyMove {
        LegacyMove {
            from: from.to_string(),
            to: to.to_string(),
        }
    }
}

/// What the first launch should do: nothing, or move these into `folder`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LegacyPlan {
    /// The folder the moves land in, relative to the data directory; None when there is nothing
    /// to move.
    pub folder: Option<String>,
    pub moves: Vec<LegacyMove>,
}

impl LegacyPlan {
    pub fn is_empty(&self) -> bool {
        self.moves.is_empty()
    }
}

/// A move that did not happen, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyFailure {
    pub from: String,
    pub error: String,
}

/// What the executor did.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LegacyOutcome {
    pub moved: Vec<LegacyMove>,
    pub failures: Vec<LegacyFailure>,
    /// Whether the marker could be written. False means the next launch will plan again.
    pub marker_written: bool,
}

pub struct LegacyData;

impl LegacyData {
    /// Written into the data directory on the native app's first launch. Its presence means the
    /// files there are the native app's own, whatever they are called.
    pub const MARKER_FILE_NAME: &'static str = "native.marker";

    const MARKER_TEXT: &'static str = "This directory is used by the native BertBrowser. \
        The files of the earlier (C#) version were moved into a legacy-<date> folder beside this \
        file on the first native launch, and are not read or deleted by it.\n";

    /// Whether an entry is one the C# app wrote: its database (`bertbrowser.db` and anything
    /// `bertbrowser.db*` — journal, WAL, backup), `settings.json`, and the `themes` and
    /// `templates` folders. A file named like one of the folders, or a folder named like one of
    /// the files, is something else.
    pub fn is_legacy_name(name: &str, is_directory: bool) -> bool {
        let lower = name.to_lowercase();
        if is_directory {
            lower == "themes" || lower == "templates"
        } else {
            lower == "settings.json" || lower.starts_with("bertbrowser.db")
        }
    }

    /// `legacy-<yyyymmdd>` for `today`, with `-2`, `-3`… appended until `taken` says no, so a
    /// second move on one day never lands on top of the first.
    pub fn folder_name(today: FileTime, taken: impl Fn(&str) -> bool) -> String {
        let c = today.civil();
        let base = format!("legacy-{:04}{:02}{:02}", c.year, c.month, c.day);
        if !taken(&base) {
            return base;
        }
        (2..)
            .map(|n| format!("{base}-{n}"))
            .find(|candidate| !taken(candidate))
            .expect("an unbounded sequence has a free name")
    }

    /// The moves for a listing of the data directory. Nothing once the marker is present; otherwise
    /// every legacy entry into a dated folder that does not exist yet. `today` is the local
    /// wall-clock time (`FileTime::now().to_local()`), since the folder is named for the user.
    pub fn plan(entries: &[DataDirEntry], today: FileTime, marker_present: bool) -> LegacyPlan {
        if marker_present {
            return LegacyPlan::default();
        }

        let legacy: Vec<&DataDirEntry> = entries
            .iter()
            .filter(|e| Self::is_legacy_name(&e.name, e.is_directory))
            .collect();
        if legacy.is_empty() {
            return LegacyPlan::default();
        }

        let folder = Self::folder_name(today, |candidate| {
            entries
                .iter()
                .any(|e| e.is_directory && e.name.eq_ignore_ascii_case(candidate))
        });
        let moves = legacy
            .iter()
            .map(|e| LegacyMove::new(&e.name, &format!("{folder}\\{}", e.name)))
            .collect();
        LegacyPlan {
            folder: Some(folder),
            moves,
        }
    }

    /// Carries out a plan under `data_dir` and writes the marker. Each move is checked against the
    /// disk first: a destination that already exists fails that one move (nothing is ever
    /// overwritten), and so does a source that is gone. The marker is written whatever happened,
    /// because once this app has started it may create its own files under these names, and a
    /// second pass would move *those*; what failed is in the outcome for the caller to report.
    pub fn execute(data_dir: &Path, plan: &LegacyPlan) -> LegacyOutcome {
        let mut outcome = LegacyOutcome::default();

        if let Some(folder) = &plan.folder
            && !plan.moves.is_empty()
            && let Err(e) = std::fs::create_dir_all(data_dir.join(folder))
        {
            for m in &plan.moves {
                outcome.failures.push(LegacyFailure {
                    from: m.from.clone(),
                    error: format!("could not create {folder}: {e}"),
                });
            }
            outcome.marker_written = Self::write_marker(data_dir);
            return outcome;
        }

        for m in &plan.moves {
            let from = data_dir.join(&m.from);
            let to = data_dir.join(&m.to);
            let result = if to.exists() {
                Err("the destination already exists".to_string())
            } else if !from.exists() {
                Err("the source no longer exists".to_string())
            } else {
                std::fs::rename(&from, &to).map_err(|e| e.to_string())
            };
            match result {
                Ok(()) => outcome.moved.push(m.clone()),
                Err(error) => outcome.failures.push(LegacyFailure {
                    from: m.from.clone(),
                    error,
                }),
            }
        }

        outcome.marker_written = Self::write_marker(data_dir);
        outcome
    }

    /// The whole first-launch step: creates the data directory if need be, lists it, plans, and
    /// executes. Cheap on every later launch — one directory listing and a marker check.
    pub fn migrate(data_dir: &Path, today: FileTime) -> std::io::Result<LegacyOutcome> {
        std::fs::create_dir_all(data_dir)?;
        let marker_present = data_dir.join(Self::MARKER_FILE_NAME).is_file();
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(data_dir)? {
            let entry = entry?;
            entries.push(DataDirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_directory: entry.file_type()?.is_dir(),
            });
        }
        let plan = Self::plan(&entries, today, marker_present);
        Ok(Self::execute(data_dir, &plan))
    }

    fn write_marker(data_dir: &Path) -> bool {
        std::fs::write(data_dir.join(Self::MARKER_FILE_NAME), Self::MARKER_TEXT).is_ok()
    }
}
