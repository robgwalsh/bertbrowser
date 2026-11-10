//! The tool windows' vocabulary: checksums, the file compare, disk usage, the change timeline and the
//! folder comparison's banner and sync. One entry point (`App::tools`) takes a verb and a JSON
//! argument and answers JSON, the way the `*_do` exports do, so a window's whole vocabulary is one
//! function and the host holds no state of its own.
//!
//! Every verb here blocks for as long as the files take. The host calls them from a worker.

pub(crate) mod changes;
pub(crate) mod checksum;
pub(crate) mod compare_view;
pub(crate) mod disk;
pub(crate) mod duplicates;
pub(crate) mod filecompare;

use std::collections::HashSet;
use std::sync::Mutex;

use bb_core::checksums::ChecksumAlgorithm;
use serde_json::{Value, json};

use crate::app::App;
use crate::app::lock;
use crate::ids::{AppError, AppResult};
use crate::tools::changes::ChangesSession;
use crate::tools::checksum::{ChecksumSession, parse_algorithms, write_checksum_file};
use crate::tools::filecompare::FileCompareSession;

#[derive(Default)]
pub(crate) struct ToolsState {
    pub(crate) checksum: Mutex<Option<ChecksumSession>>,
    filecompare: Mutex<Option<FileCompareSession>>,
    changes: Mutex<Option<ChangesSession>>,
    /// The last duplicate scan's groups, by path: what "never every copy" is checked against.
    pub(crate) duplicate_groups: Mutex<Vec<Vec<String>>>,
    /// The duplicate finder's scan and what it found.
    pub(crate) duplicates: Mutex<crate::tools::duplicates::DuplicatesSession>,
    /// Disk usage's folder, read off the UI thread.
    pub(crate) disk: Mutex<crate::tools::disk::DiskLoad>,
    /// The portable devices, once read: None until the first read lands.
    pub(crate) devices: Mutex<Option<Vec<crate::seams::PortableDevice>>>,
    devices_reading: std::sync::atomic::AtomicBool,
    /// The rename box's tag reads, by path and modified time: a template is re-planned on every key,
    /// and the tags do not change under it. Spec: RenameDialog (reads tags once).
    pub(crate) rename_tags: Mutex<RenameTagCache>,
}

pub(crate) type RenameTagCache = std::collections::HashMap<
    String,
    (
        Option<bb_core::time::FileTime>,
        std::collections::HashMap<bb_core::metadata::MetadataField, String>,
    ),
>;

fn invalid(text: impl Into<String>) -> AppError {
    AppError::Invalid(text.into())
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn algorithm(v: &Value) -> AppResult<ChecksumAlgorithm> {
    v.as_str()
        .and_then(ChecksumAlgorithm::parse_ignore_case)
        .ok_or_else(|| invalid(format!("'{v}' is not an algorithm.")))
}

fn need<'a>(v: &'a Value, key: &str) -> AppResult<&'a str> {
    v[key]
        .as_str()
        .ok_or_else(|| invalid(format!("'{key}' is required.")))
}

verb_enum! {
    /// Every verb `App::tools` answers.
    ToolVerb {
        DuplicatesRemovable = "duplicates.removable",
        DuplicatesScan = "duplicates.scan",
        DuplicatesCancel = "duplicates.cancel",
        DuplicatesModel = "duplicates.model",
        SearchSaveSeed = "search.save-seed",
        SearchEditSeed = "search.edit-seed",
        SearchSaveCheck = "search.save-check",
        SearchSave = "search.save",
        WorkspaceRenameCheck = "workspace.rename-check",
        PropertiesModel = "properties.model",
        PropertiesApply = "properties.apply",
        DuplicatesLoad = "duplicates.load",
        Reveal = "reveal",
        DuplicatesPrune = "duplicates.prune",
        ChecksumHash = "checksum.hash",
        ChecksumVerify = "checksum.verify",
        ChecksumAlgorithms = "checksum.algorithms",
        ChecksumExpected = "checksum.expected",
        ChecksumRehash = "checksum.rehash",
        ChecksumStop = "checksum.stop",
        ChecksumAlgorithmOf = "checksum.algorithm-of",
        ChecksumModel = "checksum.model",
        ChecksumSave = "checksum.save",
        ChecksumWriteFile = "checksum.write-file",
        FilecompareOpen = "filecompare.open",
        FilecompareOptions = "filecompare.options",
        FilecompareStep = "filecompare.step",
        FilecompareSwap = "filecompare.swap",
        FilecompareModel = "filecompare.model",
        DiskusageModel = "diskusage.model",
        DiskusageLoad = "diskusage.load",
        DiskusageState = "diskusage.state",
        DevicesList = "devices.list",
        DevicesOpen = "devices.open",
        TreemapLayout = "treemap.layout",
        ChangesOpen = "changes.open",
        ChangesSet = "changes.set",
        ChangesMark = "changes.mark",
        ChangesModel = "changes.model",
        ChangesCount = "changes.count",
        ChangesStamp = "changes.stamp",
        ChangesClear = "changes.clear",
        ChangesSeed = "changes.seed",
        CompareModel = "compare.model",
        CompareFilter = "compare.filter",
        CompareRescan = "compare.rescan",
        CompareEnd = "compare.end",
        CompareSyncPreview = "compare.sync-preview",
        CompareSync = "compare.sync",
    }
}

impl App {
    /// Runs one tool verb. `args` is a JSON object (or empty).
    pub fn tools(&self, verb: &str, args: &str) -> AppResult<String> {
        let args: Value = if args.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(args).map_err(|e| invalid(format!("not JSON: {e}")))?
        };
        let verb: ToolVerb = verb
            .parse()
            .map_err(|()| invalid(format!("unknown tool verb '{verb}'")))?;
        let inner = &self.inner;
        let out: Value = match verb {
            ToolVerb::DuplicatesRemovable => self.tool_duplicates_removable(&args),
            ToolVerb::DuplicatesScan => self.tool_duplicates_scan(&args)?,
            ToolVerb::DuplicatesCancel => {
                inner.duplicates_cancel();
                inner.duplicates_model()
            }
            ToolVerb::DuplicatesModel => inner.duplicates_model(),
            ToolVerb::SearchSaveSeed => self.tool_search_save_seed()?,
            ToolVerb::SearchEditSeed => self.tool_search_edit_seed(&args)?,
            ToolVerb::SearchSaveCheck => self.tool_search_save_check(&args),
            ToolVerb::SearchSave => self.tool_search_save(&args)?,
            ToolVerb::WorkspaceRenameCheck => self.tool_workspace_rename_check(&args),
            ToolVerb::PropertiesModel => inner.properties_json(&strings(&args["paths"]), None),
            ToolVerb::PropertiesApply => self.tool_properties_apply(&args),
            ToolVerb::DuplicatesLoad => self.tool_duplicates_load(&args),
            ToolVerb::Reveal => self.tool_reveal(&args)?,
            ToolVerb::DuplicatesPrune => self.tool_duplicates_prune(&args),
            ToolVerb::ChecksumHash => self.tool_checksum_hash(&args)?,
            ToolVerb::ChecksumVerify => self.tool_checksum_verify(&args)?,
            ToolVerb::ChecksumAlgorithms => self.tool_checksum_algorithms(&args)?,
            ToolVerb::ChecksumExpected => self.tool_checksum_expected(&args)?,
            // Read everything again, as the window's Hash button does.
            ToolVerb::ChecksumRehash => {
                with_checksum(self, |s| s.run(true))?;
                inner.checksum_kick();
                with_checksum(self, |_| {})?
            }
            ToolVerb::ChecksumStop => with_checksum(self, |s| s.stop())?,
            ToolVerb::ChecksumAlgorithmOf => self.tool_checksum_algorithm_of(&args),
            ToolVerb::ChecksumModel => with_checksum(self, |_| {})?,
            ToolVerb::ChecksumSave => self.tool_checksum_save(&args)?,
            ToolVerb::ChecksumWriteFile => self.tool_checksum_write_file(&args)?,
            ToolVerb::FilecompareOpen => self.tool_filecompare_open(&args)?,
            ToolVerb::FilecompareOptions => self.tool_filecompare_options(&args)?,
            ToolVerb::FilecompareStep => with_filecompare(self, |s| {
                s.step(args["direction"].as_i64().unwrap_or(1).signum() as i32);
            })?,
            ToolVerb::FilecompareSwap => self.tool_filecompare_swap()?,
            ToolVerb::FilecompareModel => with_filecompare(self, |_| {})?,
            ToolVerb::DiskusageModel => self.tool_diskusage_model(&args)?,
            ToolVerb::DiskusageLoad => self.tool_diskusage_load(&args)?,
            ToolVerb::DiskusageState => inner.disk_usage_state(),
            ToolVerb::DevicesList => self.tool_devices_list(&args),
            ToolVerb::DevicesOpen => self.tool_devices_open(&args)?,
            ToolVerb::TreemapLayout => Self::tool_treemap_layout(&args),
            ToolVerb::ChangesOpen => self.tool_changes_open(&args)?,
            ToolVerb::ChangesSet => with_changes(self, |s| s.set(&args))?,
            ToolVerb::ChangesMark => {
                inner.change_mark_now()?;
                with_changes(self, |s| s.use_mark_range())?
            }
            ToolVerb::ChangesModel => with_changes(self, |_| {})?,
            // `{}` -> `{count}`: how many changes the log holds - what the History page's switch is over.
            ToolVerb::ChangesCount => json!({ "count": inner.change_log_count() }),
            ToolVerb::ChangesStamp => inner.change_log_stamp(),
            ToolVerb::ChangesClear => {
                inner.change_log_clear()?;
                json!({"ok": true})
            }
            ToolVerb::ChangesSeed => {
                json!({"rows": inner.change_log_seed(need(&args, "root")?)?})
            }
            ToolVerb::CompareModel => inner.compare_model(),
            ToolVerb::CompareFilter => self.tool_compare_filter(&args)?,
            ToolVerb::CompareRescan => {
                inner.compare_rescan()?;
                inner.compare_model()
            }
            ToolVerb::CompareEnd => {
                inner.compare_end();
                inner.compare_model()
            }
            ToolVerb::CompareSyncPreview => self.tool_compare_sync_preview(&args)?,
            ToolVerb::CompareSync => self.tool_compare_sync(&args)?,
        };
        Ok(out.to_string())
    }

    // ---- duplicates ----

    /// `{paths}` ticked -> `{paths}` a delete may take: never every copy of a group.
    fn tool_duplicates_removable(&self, args: &Value) -> Value {
        json!({ "paths": self.duplicates_removable(&strings(&args["paths"])) })
    }

    /// `{root?, minSize?, skipSystem?}`: a new scan, off the UI thread; no root is this PC.
    fn tool_duplicates_scan(&self, args: &Value) -> AppResult<Value> {
        let inner = &self.inner;
        inner.duplicates_scan(
            args["root"].as_str().map(str::to_string),
            args["minSize"].as_i64(),
            args["skipSystem"].as_bool(),
        )?;
        Ok(inner.duplicates_model())
    }

    /// `{root?}`: the window opened on this folder (none: this PC), nothing scanned yet.
    fn tool_duplicates_load(&self, args: &Value) -> Value {
        let inner = &self.inner;
        inner.duplicates_load(args["root"].as_str().map(str::to_string));
        inner.duplicates_model()
    }

    /// `{paths}` the delete took: out of the groups the window and the guard both read.
    fn tool_duplicates_prune(&self, args: &Value) -> Value {
        let inner = &self.inner;
        inner.duplicates_prune(&strings(&args["paths"]));
        inner.duplicates_model()
    }

    // ---- saving a search ----

    /// `{}` -> `{name, query, scope: current|pc, folder}` the Save search dialog starts from: the
    /// tab's search, a whole-PC one scoped to this PC, and the folder to pin it to (null inside an
    /// archive or with no folder). Spec: SavedSearchesViewModel.SeedFor.
    fn tool_search_save_seed(&self) -> AppResult<Value> {
        let inner = &self.inner;
        let tab = lock(&inner.shell).active_tab_id().ok_or(AppError::Stale)?;
        let cell = inner.tab(tab)?;
        let (query, path, global) = {
            let t = lock(&cell);
            (
                t.active_search_text().to_string(),
                t.path.clone(),
                t.is_global_search(),
            )
        };
        let pinnable = (!path.is_empty() && !crate::commands::ArchivePathCheck::in_archive(&path))
            .then_some(path);
        Ok(json!({
            "name": bb_core::rules::SavedSearchRules::default_name(&query),
            "query": query,
            "scope": if global { "pc" } else { "current" },
            "folder": pinnable,
        }))
    }

    /// `{name}` -> the seed for editing a saved search: its own values, and the tab's folder offered
    /// for pinning when it has none of its own. Spec: SavedSearchesViewModel.SeedFor(item).
    fn tool_search_edit_seed(&self, args: &Value) -> AppResult<Value> {
        let inner = &self.inner;
        let name = args["name"].as_str().unwrap_or("");
        let saved = inner
            .saved_searches()
            .into_iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| invalid("That saved search is not there any more."))?;
        let tab = lock(&inner.shell).active_tab_id();
        let here = tab
            .and_then(|t| inner.tab(t).ok())
            .map(|c| lock(&c).path.clone())
            .unwrap_or_default();
        let pinnable = (!here.is_empty() && !crate::commands::ArchivePathCheck::in_archive(&here))
            .then_some(here);
        Ok(json!({
            "name": saved.name,
            "query": saved.query,
            "scope": match saved.scope {
                bb_core::data::SavedSearchScope::Folder => "folder",
                bb_core::data::SavedSearchScope::ThisPc => "pc",
                _ => "current",
            },
            "folder": saved.scope_path.clone().or(pinnable),
        }))
    }

    /// `{name, query, scope, folder?, previous?}` -> `{problem, replacing}`, checked as it is typed.
    /// A new search may take a name already saved: it replaces that one, and the button says so.
    /// An edit (`previous`) may keep its own name but not take another's.
    fn tool_search_save_check(&self, args: &Value) -> Value {
        let inner = &self.inner;
        let (name, query, scope, folder) = saved_search_args(args);
        let previous = args["previous"].as_str();
        let replacing =
            previous.is_none() && !name.trim().is_empty() && inner.search_name_taken(name.trim());
        json!({
            "problem": inner.search_problem(name, query, scope, folder, Some(previous.unwrap_or(name.trim()))),
            "replacing": replacing,
        })
    }

    fn tool_search_save(&self, args: &Value) -> AppResult<Value> {
        let inner = &self.inner;
        let (name, query, scope, folder) = saved_search_args(args);
        // An edit renames first, so the stored casing is the one typed.
        if let Some(previous) = args["previous"]
            .as_str()
            .filter(|p| !p.eq_ignore_ascii_case(name.trim()))
        {
            inner.search_rename(previous, name.trim())?;
        }
        inner.search_save(name, query.trim(), scope, folder)?;
        Ok(json!({ "ok": true }))
    }

    /// `{old, name}` -> `{problem}` for renaming a workspace.
    fn tool_workspace_rename_check(&self, args: &Value) -> Value {
        let old = args["old"].as_str().unwrap_or("");
        json!({ "problem": self.inner.workspace_rename_problem(old, args["name"].as_str().unwrap_or("")) })
    }

    // ---- properties ----

    /// `{paths, readOnly?, hidden?}`: set the attributes; a flag left out is not touched.
    fn tool_properties_apply(&self, args: &Value) -> Value {
        self.inner.properties_apply(
            &strings(&args["paths"]),
            args["readOnly"].as_bool(),
            args["hidden"].as_bool(),
        )
    }

    /// `{path, isDirectory}`: what a tool window's double-click does - a folder opens in a tab
    /// beside this one, a file is shown selected where it lives. Spec: MainWindow.RevealFromDiskUsage.
    fn tool_reveal(&self, args: &Value) -> AppResult<Value> {
        let inner = &self.inner;
        let path = args["path"].as_str().unwrap_or("").to_string();
        if path.is_empty() {
            return Err(invalid("reveal needs a path"));
        }
        let (pane, tab) = {
            let shell = lock(&inner.shell);
            (shell.active_pane, shell.active_tab_id())
        };
        if args["isDirectory"].as_bool().unwrap_or(false) {
            inner.open_tab(pane, &path, false)?;
        } else if let Some(tab) = tab {
            inner.reveal_file(tab, &path)?;
        }
        Ok(json!({}))
    }

    // ---- checksums ----

    fn tool_checksum_hash(&self, args: &Value) -> AppResult<Value> {
        let inner = &self.inner;
        let state = &inner.extra.tools;
        let selected = match args.get("algorithms").filter(|a| a.is_array()) {
            Some(list) => crate::tools::checksum::parse_algorithms(&strings(list).join(",")),
            None => parse_algorithms(&lock(&inner.settings).checksum_algorithms),
        };
        // The old window's reading, if any, is not this one's.
        if let Some(old) = lock(&state.checksum).as_mut() {
            old.stop();
        }
        *lock(&state.checksum) = Some(ChecksumSession::hash(&strings(&args["paths"]), selected));
        inner.checksum_kick();
        with_checksum(self, |_| {})
    }

    fn tool_checksum_verify(&self, args: &Value) -> AppResult<Value> {
        let inner = &self.inner;
        let state = &inner.extra.tools;
        let selected = parse_algorithms(&lock(&inner.settings).checksum_algorithms);
        if let Some(old) = lock(&state.checksum).as_mut() {
            old.stop();
        }
        *lock(&state.checksum) = Some(ChecksumSession::verify(need(args, "path")?, selected));
        inner.checksum_kick();
        with_checksum(self, |_| {})
    }

    fn tool_checksum_algorithms(&self, args: &Value) -> AppResult<Value> {
        with_checksum(self, |s| {
            s.set_algorithms(parse_algorithms(&strings(&args["algorithms"]).join(",")));
        })?;
        self.inner.checksum_kick();
        with_checksum(self, |_| {})
    }

    fn tool_checksum_expected(&self, args: &Value) -> AppResult<Value> {
        with_checksum(self, |s| {
            s.set_expected(args["text"].as_str().unwrap_or(""))
        })?;
        self.inner.checksum_kick();
        with_checksum(self, |_| {})
    }

    /// `{path}` -> `{algorithm}`: what a checksum file's name says it holds, or null for a
    /// name that is not one. A single one selected opens Verify rather than being hashed.
    fn tool_checksum_algorithm_of(&self, args: &Value) -> Value {
        let a = bb_core::checksums::ChecksumAlgorithms::from_extension(args["path"].as_str());
        json!({ "algorithm": a.map(|a| a.name()) })
    }

    fn tool_checksum_save(&self, args: &Value) -> AppResult<Value> {
        let algorithm = algorithm(&args["algorithm"])?;
        let guard = lock(&self.inner.extra.tools.checksum);
        let session = guard
            .as_ref()
            .ok_or_else(|| invalid("There is no checksum window."))?;
        Ok(match session.save(need(args, "path")?, algorithm) {
            Ok(note) => json!({"ok": true, "note": note}),
            Err(error) => json!({"ok": false, "error": error}),
        })
    }

    fn tool_checksum_write_file(&self, args: &Value) -> AppResult<Value> {
        write_checksum_file(
            need(args, "path")?,
            algorithm(&args["algorithm"])?,
            &strings(&args["files"]),
        )?;
        Ok(json!({"ok": true}))
    }

    // ---- comparing two files ----
    // Each of these begins a comparison on a worker; `Model::FileCompare` says it landed.

    fn tool_filecompare_open(&self, args: &Value) -> AppResult<Value> {
        let inner = &self.inner;
        let mut session = FileCompareSession::pending(need(args, "left")?, need(args, "right")?);
        let job = session.begin();
        let model = session.model().clone();
        *lock(&inner.extra.tools.filecompare) = Some(session);
        inner.filecompare_run(job);
        Ok(model)
    }

    fn tool_filecompare_options(&self, args: &Value) -> AppResult<Value> {
        let job = {
            let mut guard = lock(&self.inner.extra.tools.filecompare);
            let s = guard
                .as_mut()
                .ok_or_else(|| invalid("Nothing has been compared."))?;
            s.set_options(
                args["ignoreWhitespace"].as_bool().unwrap_or(true),
                args["ignoreCase"].as_bool().unwrap_or(false),
                args["forceText"].as_bool().unwrap_or(false),
            );
            s.begin()
        };
        self.inner.filecompare_run(job);
        with_filecompare(self, |_| {})
    }

    fn tool_filecompare_swap(&self) -> AppResult<Value> {
        let job = {
            let mut guard = lock(&self.inner.extra.tools.filecompare);
            let s = guard
                .as_mut()
                .ok_or_else(|| invalid("Nothing has been compared."))?;
            s.swap();
            s.begin()
        };
        self.inner.filecompare_run(job);
        with_filecompare(self, |_| {})
    }

    // ---- disk usage ----

    fn tool_diskusage_model(&self, args: &Value) -> AppResult<Value> {
        let path = args["path"].as_str().unwrap_or("");
        crate::tools::disk::require_dir(path)?;
        self.inner.disk_usage_model(path)
    }

    /// `{path}`: reads it on a worker (a later load supersedes it), and says so with
    /// `Model::DiskUsage`; `diskusage.state` is `{path, loading, model, error}`.
    fn tool_diskusage_load(&self, args: &Value) -> AppResult<Value> {
        let path = args["path"].as_str().unwrap_or("");
        crate::tools::disk::require_dir(path)?;
        self.inner.disk_usage_load(path.to_string());
        Ok(self.inner.disk_usage_state())
    }

    // ---- portable devices ----

    /// `{loaded, devices:[{name, path}]}`; the first ask starts the read (off this thread, it can
    /// take seconds) and `Model::Devices` says when it lands. `{refresh:true}` reads again.
    fn tool_devices_list(&self, args: &Value) -> Value {
        let inner = &self.inner;
        let state = &inner.extra.tools;
        let refresh = args["refresh"].as_bool().unwrap_or(false);
        let have = lock(&state.devices).clone();
        if (have.is_none() || refresh)
            && !state
                .devices_reading
                .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            // Not "loading": the read can take seconds, and the folders are not waiting on it.
            inner.schedule_quiet(|inner| {
                let found = inner.seams.devices.list();
                *lock(&inner.extra.tools.devices) = Some(found);
                inner
                    .extra
                    .tools
                    .devices_reading
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                inner.hub.model(crate::events::Model::Devices);
            });
        }
        json!({
            "loaded": have.is_some(),
            "devices": have.unwrap_or_default().iter()
                .map(|d| json!({ "name": d.name, "path": d.shell_path }))
                .collect::<Vec<_>>(),
        })
    }

    /// `{path}`: Explorer on the device, through the one launcher. Spec: PortableDevices.OpenInExplorer.
    fn tool_devices_open(&self, args: &Value) -> AppResult<Value> {
        let path = need(args, "path")?;
        let windows = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
        // The full path, so a stray explorer.exe earlier on PATH cannot decide what opens.
        let explorer = format!("{}\\explorer.exe", windows.trim_end_matches('\\'));
        let request = bb_shell::LaunchRequest {
            file: explorer,
            arguments: format!("\"{path}\""),
            ..bb_shell::LaunchRequest::default()
        };
        Ok(match self.inner.seams.launcher.start(&request) {
            Ok(()) => json!({ "ok": true }),
            Err(e) => json!({ "ok": false, "error": e }),
        })
    }

    fn tool_treemap_layout(args: &Value) -> Value {
        let weights: Vec<f64> = args["weights"]
            .as_array()
            .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect())
            .unwrap_or_default();
        crate::tools::disk::treemap_layout(
            &weights,
            args["width"].as_f64().unwrap_or(0.0),
            args["height"].as_f64().unwrap_or(0.0),
        )
    }

    // ---- the change timeline ----

    fn tool_changes_open(&self, args: &Value) -> AppResult<Value> {
        let inner = &self.inner;
        let hidden = lock(&inner.settings).show_hidden_items;
        // Inside an archive the timeline watches the archive's own folder: the log records
        // real paths, and an entry is not one. Spec: ShellViewModel.OpenChanges.
        let scope = bb_core::changes::ChangeLogRules::scope_for(args["path"].as_str(), |p| {
            std::path::Path::new(p).is_file()
                && bb_core::archives::ArchiveFormats::is_archive_name(bb_paths::file_name(p))
        });
        *lock(&inner.extra.tools.changes) = Some(ChangesSession::new(scope, hidden));
        with_changes(self, |_| {})
    }

    // ---- the folder comparison ----

    fn tool_compare_filter(&self, args: &Value) -> AppResult<Value> {
        let inner = &self.inner;
        inner.compare_set_differences_only(args["on"].as_bool().unwrap_or(true))?;
        Ok(inner.compare_model())
    }

    fn tool_compare_sync_preview(&self, args: &Value) -> AppResult<Value> {
        let ticked = ticked_keys(args);
        self.inner.sync_preview_json(
            args["withDeletes"].as_bool().unwrap_or(false),
            ticked.as_ref(),
        )
    }

    fn tool_compare_sync(&self, args: &Value) -> AppResult<Value> {
        let op = self.inner.compare_sync(
            args["withDeletes"].as_bool().unwrap_or(false),
            ticked_keys(args),
        )?;
        Ok(json!({"op": op.0}))
    }
}

fn ticked_keys(args: &Value) -> Option<HashSet<String>> {
    args.get("ticked")
        .filter(|t| t.is_array())
        .map(|t| strings(t).into_iter().collect())
}

/// A saved search as the dialog states it: the folder only counts for a pinned scope.
fn saved_search_args(args: &Value) -> (&str, &str, bb_core::data::SavedSearchScope, Option<&str>) {
    use bb_core::data::SavedSearchScope;
    let scope = match args["scope"].as_str().unwrap_or("") {
        "folder" => SavedSearchScope::Folder,
        "pc" => SavedSearchScope::ThisPc,
        _ => SavedSearchScope::CurrentFolder,
    };
    let folder = (scope == SavedSearchScope::Folder)
        .then(|| args["folder"].as_str())
        .flatten();
    (
        args["name"].as_str().unwrap_or(""),
        args["query"].as_str().unwrap_or(""),
        scope,
        folder,
    )
}

fn with_checksum(app: &App, change: impl FnOnce(&mut ChecksumSession)) -> AppResult<Value> {
    let mut guard = lock(&app.inner.extra.tools.checksum);
    let session = guard
        .as_mut()
        .ok_or_else(|| invalid("There is no checksum window."))?;
    change(session);
    Ok(session.model())
}

impl crate::app::Inner {
    fn filecompare_run(self: &std::sync::Arc<Self>, job: crate::tools::filecompare::CompareJob) {
        self.hub.model(crate::events::Model::FileCompare);
        self.schedule(move |inner| {
            let (model, hunks) = FileCompareSession::compute(&job);
            let landed = lock(&inner.extra.tools.filecompare)
                .as_mut()
                .is_some_and(|s| s.apply(job.generation, model, hunks));
            if landed {
                inner.hub.model(crate::events::Model::FileCompare);
            }
        });
    }
}

fn with_filecompare(app: &App, change: impl FnOnce(&mut FileCompareSession)) -> AppResult<Value> {
    let mut guard = lock(&app.inner.extra.tools.filecompare);
    let session = guard
        .as_mut()
        .ok_or_else(|| invalid("Nothing has been compared."))?;
    change(session);
    Ok(session.model().clone())
}

fn with_changes(app: &App, change: impl FnOnce(&mut ChangesSession)) -> AppResult<Value> {
    let mut guard = lock(&app.inner.extra.tools.changes);
    let session = guard
        .as_mut()
        .ok_or_else(|| invalid("The timeline is not open."))?;
    change(session);
    Ok(session.model(&app.inner))
}
