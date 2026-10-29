//! The tool windows' vocabulary: checksums, the file compare, disk usage, the change timeline and the
//! folder comparison's banner and sync. One entry point (`App::tools`) takes a verb and a JSON
//! argument and answers JSON, the way the `*_do` exports do, so a window's whole vocabulary is one
//! function and the host holds no state of its own.
//!
//! Every verb here blocks for as long as the files take. The host calls them from a worker.

use std::collections::HashSet;
use std::sync::Mutex;

use bb_core::checksums::ChecksumAlgorithm;
use serde_json::{Value, json};

use crate::app::App;
use crate::app::lock;
use crate::ids::{AppError, AppResult};
use crate::tools_changes::ChangesSession;
use crate::tools_checksum::{ChecksumSession, parse_algorithms, write_checksum_file};
use crate::tools_filecompare::FileCompareSession;

#[derive(Default)]
pub(crate) struct ToolsState {
    pub(crate) checksum: Mutex<Option<ChecksumSession>>,
    filecompare: Mutex<Option<FileCompareSession>>,
    changes: Mutex<Option<ChangesSession>>,
    /// The last duplicate scan's groups, by path: what "never every copy" is checked against.
    pub(crate) duplicate_groups: Mutex<Vec<Vec<String>>>,
    /// The duplicate finder's scan and what it found.
    pub(crate) duplicates: Mutex<crate::tools_duplicates::DuplicatesSession>,
    /// Disk usage's folder, read off the UI thread.
    pub(crate) disk: Mutex<crate::tools_disk::DiskLoad>,
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

impl App {
    /// Runs one tool verb. `args` is a JSON object (or empty).
    pub fn tools(&self, verb: &str, args: &str) -> AppResult<String> {
        let args: Value = if args.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(args).map_err(|e| invalid(format!("not JSON: {e}")))?
        };
        let inner = &self.inner;
        let state = &inner.extra.tools;
        let out: Value = match verb {
            // ---- duplicates ----
            // `{paths}` ticked -> `{paths}` a delete may take: never every copy of a group.
            "duplicates.removable" => {
                json!({ "paths": self.duplicates_removable(&strings(&args["paths"])) })
            }
            // `{root?, minSize?, skipSystem?}`: a new scan, off the UI thread; no root is this PC.
            "duplicates.scan" => {
                inner.duplicates_scan(
                    args["root"].as_str().map(str::to_string),
                    args["minSize"].as_i64(),
                    args["skipSystem"].as_bool(),
                )?;
                inner.duplicates_model()
            }
            "duplicates.cancel" => {
                inner.duplicates_cancel();
                inner.duplicates_model()
            }
            "duplicates.model" => inner.duplicates_model(),
            // ---- saving a search ----
            // `{}` -> `{name, query, scope: current|pc, folder}` the Save search dialog starts from: the
            // tab's search, a whole-PC one scoped to this PC, and the folder to pin it to (null inside an
            // archive or with no folder). Spec: SavedSearchesViewModel.SeedFor.
            "search.save-seed" => {
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
                let pinnable = (!path.is_empty()
                    && !crate::commands::ArchivePathCheck::in_archive(&path))
                .then_some(path);
                json!({
                    "name": bb_core::rules::SavedSearchRules::default_name(&query),
                    "query": query,
                    "scope": if global { "pc" } else { "current" },
                    "folder": pinnable,
                })
            }
            // `{name}` -> the seed for editing a saved search: its own values, and the tab's folder offered
            // for pinning when it has none of its own. Spec: SavedSearchesViewModel.SeedFor(item).
            "search.edit-seed" => {
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
                let pinnable = (!here.is_empty()
                    && !crate::commands::ArchivePathCheck::in_archive(&here))
                .then_some(here);
                json!({
                    "name": saved.name,
                    "query": saved.query,
                    "scope": match saved.scope {
                        bb_core::data::SavedSearchScope::Folder => "folder",
                        bb_core::data::SavedSearchScope::ThisPc => "pc",
                        _ => "current",
                    },
                    "folder": saved.scope_path.clone().or(pinnable),
                })
            }
            // `{name, query, scope, folder?, previous?}` -> `{problem, replacing}`, checked as it is typed.
            // A new search may take a name already saved: it replaces that one, and the button says so.
            // An edit (`previous`) may keep its own name but not take another's.
            "search.save-check" => {
                let (name, query, scope, folder) = saved_search_args(&args);
                let previous = args["previous"].as_str();
                let replacing = previous.is_none()
                    && !name.trim().is_empty()
                    && inner.search_name_taken(name.trim());
                json!({
                    "problem": inner.search_problem(name, query, scope, folder, Some(previous.unwrap_or(name.trim()))),
                    "replacing": replacing,
                })
            }
            "search.save" => {
                let (name, query, scope, folder) = saved_search_args(&args);
                // An edit renames first, so the stored casing is the one typed.
                if let Some(previous) = args["previous"]
                    .as_str()
                    .filter(|p| !p.eq_ignore_ascii_case(name.trim()))
                {
                    inner.search_rename(previous, name.trim())?;
                }
                inner.search_save(name, query.trim(), scope, folder)?;
                json!({ "ok": true })
            }
            // `{old, name}` -> `{problem}` for renaming a workspace.
            "workspace.rename-check" => {
                let old = args["old"].as_str().unwrap_or("");
                json!({ "problem": inner.workspace_rename_problem(old, args["name"].as_str().unwrap_or("")) })
            }
            // ---- properties ----
            // `{paths}`: what the Properties dialog shows for these items.
            "properties.model" => inner.properties_json(&strings(&args["paths"]), None),
            // `{paths, readOnly?, hidden?}`: set the attributes; a flag left out is not touched.
            "properties.apply" => inner.properties_apply(
                &strings(&args["paths"]),
                args["readOnly"].as_bool(),
                args["hidden"].as_bool(),
            ),
            // `{root?}`: the window opened on this folder (none: this PC), nothing scanned yet.
            "duplicates.load" => {
                inner.duplicates_load(args["root"].as_str().map(str::to_string));
                inner.duplicates_model()
            }
            // `{path, isDirectory}`: what a tool window's double-click does - a folder opens in a tab
            // beside this one, a file is shown selected where it lives. Spec: MainWindow.RevealFromDiskUsage.
            "reveal" => {
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
                json!({})
            }
            // `{paths}` the delete took: out of the groups the window and the guard both read.
            "duplicates.prune" => {
                inner.duplicates_prune(&strings(&args["paths"]));
                inner.duplicates_model()
            }
            // ---- checksums ----
            "checksum.hash" => {
                let selected = match args.get("algorithms").filter(|a| a.is_array()) {
                    Some(list) => crate::tools_checksum::parse_algorithms(&strings(list).join(",")),
                    None => parse_algorithms(&lock(&inner.settings).checksum_algorithms),
                };
                // The old window's reading, if any, is not this one's.
                if let Some(old) = lock(&state.checksum).as_mut() {
                    old.stop();
                }
                *lock(&state.checksum) =
                    Some(ChecksumSession::hash(&strings(&args["paths"]), selected));
                inner.checksum_kick();
                with_checksum(self, |_| {})?
            }
            "checksum.verify" => {
                let selected = parse_algorithms(&lock(&inner.settings).checksum_algorithms);
                if let Some(old) = lock(&state.checksum).as_mut() {
                    old.stop();
                }
                *lock(&state.checksum) =
                    Some(ChecksumSession::verify(need(&args, "path")?, selected));
                inner.checksum_kick();
                with_checksum(self, |_| {})?
            }
            "checksum.algorithms" => {
                with_checksum(self, |s| {
                    s.set_algorithms(parse_algorithms(&strings(&args["algorithms"]).join(",")));
                })?;
                inner.checksum_kick();
                with_checksum(self, |_| {})?
            }
            "checksum.expected" => {
                with_checksum(self, |s| {
                    s.set_expected(args["text"].as_str().unwrap_or(""))
                })?;
                inner.checksum_kick();
                with_checksum(self, |_| {})?
            }
            // Read everything again, as the window's Hash button does.
            "checksum.rehash" => {
                with_checksum(self, |s| s.run(true))?;
                inner.checksum_kick();
                with_checksum(self, |_| {})?
            }
            "checksum.stop" => with_checksum(self, |s| s.stop())?,
            // `{path}` -> `{algorithm}`: what a checksum file's name says it holds, or null for a
            // name that is not one. A single one selected opens Verify rather than being hashed.
            "checksum.algorithm-of" => {
                let a =
                    bb_core::checksums::ChecksumAlgorithms::from_extension(args["path"].as_str());
                json!({ "algorithm": a.map(|a| a.name()) })
            }
            "checksum.model" => with_checksum(self, |_| {})?,
            "checksum.save" => {
                let algorithm = algorithm(&args["algorithm"])?;
                let guard = lock(&state.checksum);
                let session = guard
                    .as_ref()
                    .ok_or_else(|| invalid("There is no checksum window."))?;
                match session.save(need(&args, "path")?, algorithm) {
                    Ok(note) => json!({"ok": true, "note": note}),
                    Err(error) => json!({"ok": false, "error": error}),
                }
            }
            "checksum.write-file" => {
                write_checksum_file(
                    need(&args, "path")?,
                    algorithm(&args["algorithm"])?,
                    &strings(&args["files"]),
                )?;
                json!({"ok": true})
            }

            // ---- comparing two files ----
            // Each of these begins a comparison on a worker; `Model::FileCompare` says it landed.
            "filecompare.open" => {
                let mut session =
                    FileCompareSession::pending(need(&args, "left")?, need(&args, "right")?);
                let job = session.begin();
                let model = session.model().clone();
                *lock(&state.filecompare) = Some(session);
                inner.filecompare_run(job);
                model
            }
            "filecompare.options" => {
                let job = {
                    let mut guard = lock(&state.filecompare);
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
                inner.filecompare_run(job);
                with_filecompare(self, |_| {})?
            }
            "filecompare.step" => with_filecompare(self, |s| {
                s.step(args["direction"].as_i64().unwrap_or(1).signum() as i32);
            })?,
            "filecompare.swap" => {
                let job = {
                    let mut guard = lock(&state.filecompare);
                    let s = guard
                        .as_mut()
                        .ok_or_else(|| invalid("Nothing has been compared."))?;
                    s.swap();
                    s.begin()
                };
                inner.filecompare_run(job);
                with_filecompare(self, |_| {})?
            }
            "filecompare.model" => with_filecompare(self, |_| {})?,

            // ---- disk usage ----
            "diskusage.model" => {
                let path = args["path"].as_str().unwrap_or("");
                crate::tools_disk::require_dir(path)?;
                inner.disk_usage_model(path)?
            }
            // `{path}`: reads it on a worker (a later load supersedes it), and says so with
            // `Model::DiskUsage`; `diskusage.state` is `{path, loading, model, error}`.
            "diskusage.load" => {
                let path = args["path"].as_str().unwrap_or("");
                crate::tools_disk::require_dir(path)?;
                inner.disk_usage_load(path.to_string());
                inner.disk_usage_state()
            }
            "diskusage.state" => inner.disk_usage_state(),

            // ---- portable devices ----
            // `{loaded, devices:[{name, path}]}`; the first ask starts the read (off this thread, it can
            // take seconds) and `Model::Devices` says when it lands. `{refresh:true}` reads again.
            "devices.list" => {
                let refresh = args["refresh"].as_bool().unwrap_or(false);
                let have = lock(&state.devices).clone();
                if (have.is_none() || refresh)
                    && !state
                        .devices_reading
                        .swap(true, std::sync::atomic::Ordering::SeqCst)
                {
                    inner.schedule(|inner| {
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
            // `{path}`: Explorer on the device, through the one launcher. Spec: PortableDevices.OpenInExplorer.
            "devices.open" => {
                let path = need(&args, "path")?;
                let windows = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
                // The full path, so a stray explorer.exe earlier on PATH cannot decide what opens.
                let explorer = format!("{}\\explorer.exe", windows.trim_end_matches('\\'));
                let request = bb_shell::LaunchRequest {
                    file: explorer,
                    arguments: format!("\"{path}\""),
                    ..bb_shell::LaunchRequest::default()
                };
                match inner.seams.launcher.start(&request) {
                    Ok(()) => json!({ "ok": true }),
                    Err(e) => json!({ "ok": false, "error": e }),
                }
            }
            "treemap.layout" => {
                let weights: Vec<f64> = args["weights"]
                    .as_array()
                    .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect())
                    .unwrap_or_default();
                crate::tools_disk::treemap_layout(
                    &weights,
                    args["width"].as_f64().unwrap_or(0.0),
                    args["height"].as_f64().unwrap_or(0.0),
                )
            }

            // ---- the change timeline ----
            "changes.open" => {
                let hidden = lock(&inner.settings).show_hidden_items;
                // Inside an archive the timeline watches the archive's own folder: the log records
                // real paths, and an entry is not one. Spec: ShellViewModel.OpenChanges.
                let scope =
                    bb_core::changes::ChangeLogRules::scope_for(args["path"].as_str(), |p| {
                        std::path::Path::new(p).is_file()
                            && bb_core::archives::ArchiveFormats::is_archive_name(
                                bb_paths::file_name(p),
                            )
                    });
                *lock(&state.changes) = Some(ChangesSession::new(scope, hidden));
                with_changes(self, |_| {})?
            }
            "changes.set" => with_changes(self, |s| s.set(&args))?,
            "changes.mark" => {
                inner.change_mark_now()?;
                with_changes(self, |s| s.use_mark_range())?
            }
            "changes.model" => with_changes(self, |_| {})?,
            // `{}` -> `{count}`: how many changes the log holds - what the History page's switch is over.
            "changes.count" => json!({ "count": inner.change_log_count() }),
            "changes.stamp" => inner.change_log_stamp(),
            "changes.clear" => {
                inner.change_log_clear()?;
                json!({"ok": true})
            }
            "changes.seed" => json!({"rows": inner.change_log_seed(need(&args, "root")?)?}),

            // ---- the folder comparison ----
            "compare.model" => inner.compare_model(),
            "compare.filter" => {
                inner.compare_set_differences_only(args["on"].as_bool().unwrap_or(true))?;
                inner.compare_model()
            }
            "compare.rescan" => {
                inner.compare_rescan()?;
                inner.compare_model()
            }
            "compare.end" => {
                inner.compare_end();
                inner.compare_model()
            }
            "compare.sync-preview" => {
                let ticked = ticked_keys(&args);
                inner.sync_preview_json(
                    args["withDeletes"].as_bool().unwrap_or(false),
                    ticked.as_ref(),
                )?
            }
            "compare.sync" => {
                let op = inner.compare_sync(
                    args["withDeletes"].as_bool().unwrap_or(false),
                    ticked_keys(&args),
                )?;
                json!({"op": op.0})
            }
            other => return Err(invalid(format!("unknown tool verb '{other}'"))),
        };
        Ok(out.to_string())
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
    fn filecompare_run(self: &std::sync::Arc<Self>, job: crate::tools_filecompare::CompareJob) {
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
