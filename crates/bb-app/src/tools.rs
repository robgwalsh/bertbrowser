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
    checksum: Mutex<Option<ChecksumSession>>,
    filecompare: Mutex<Option<FileCompareSession>>,
    changes: Mutex<Option<ChangesSession>>,
    /// The last duplicate scan's groups, by path: what "never every copy" is checked against.
    pub(crate) duplicate_groups: Mutex<Vec<Vec<String>>>,
}

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
            // ---- checksums ----
            "checksum.hash" => {
                let selected = match args.get("algorithms").filter(|a| a.is_array()) {
                    Some(list) => crate::tools_checksum::parse_algorithms(&strings(list).join(",")),
                    None => parse_algorithms(&lock(&inner.settings).checksum_algorithms),
                };
                let session = ChecksumSession::hash(&strings(&args["paths"]), selected);
                let model = session.model();
                *lock(&state.checksum) = Some(session);
                model
            }
            "checksum.verify" => {
                let selected = parse_algorithms(&lock(&inner.settings).checksum_algorithms);
                let session = ChecksumSession::verify(need(&args, "path")?, selected);
                let model = session.model();
                *lock(&state.checksum) = Some(session);
                model
            }
            "checksum.algorithms" => with_checksum(self, |s| {
                s.set_algorithms(parse_algorithms(&strings(&args["algorithms"]).join(",")));
            })?,
            "checksum.expected" => with_checksum(self, |s| {
                s.set_expected(args["text"].as_str().unwrap_or(""))
            })?,
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
            "filecompare.open" => {
                let session = FileCompareSession::new(need(&args, "left")?, need(&args, "right")?);
                let model = session.model().clone();
                *lock(&state.filecompare) = Some(session);
                model
            }
            "filecompare.options" => with_filecompare(self, |s| {
                s.set_options(
                    args["ignoreWhitespace"].as_bool().unwrap_or(true),
                    args["ignoreCase"].as_bool().unwrap_or(false),
                    args["forceText"].as_bool().unwrap_or(false),
                );
            })?,
            "filecompare.step" => with_filecompare(self, |s| {
                s.step(args["direction"].as_i64().unwrap_or(1).signum() as i32);
            })?,
            "filecompare.model" => with_filecompare(self, |_| {})?,

            // ---- disk usage ----
            "diskusage.model" => {
                let path = args["path"].as_str().unwrap_or("");
                crate::tools_disk::require_dir(path)?;
                inner.disk_usage_model(path)?
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
                let scope = args["path"].as_str().map(str::to_string);
                *lock(&state.changes) = Some(ChangesSession::new(scope, hidden));
                with_changes(self, |_| {})?
            }
            "changes.set" => with_changes(self, |s| s.set(&args))?,
            "changes.mark" => {
                inner.change_mark_now()?;
                with_changes(self, |s| s.use_mark_range())?
            }
            "changes.model" => with_changes(self, |_| {})?,
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

fn with_checksum(app: &App, change: impl FnOnce(&mut ChecksumSession)) -> AppResult<Value> {
    let mut guard = lock(&app.inner.extra.tools.checksum);
    let session = guard
        .as_mut()
        .ok_or_else(|| invalid("There is no checksum window."))?;
    change(session);
    Ok(session.model())
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
