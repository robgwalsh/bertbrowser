//! The Properties dialog: what the selection is (name, type, where, how big, when), its Read-only and
//! Hidden attributes to change, and for folders the totals the MFT pass recorded - never a walk, and
//! blank rather than zero when nothing was recorded. The shell's own property list for one file is the
//! host's to read (it is COM); this says whether it may be read at all, since a cloud placeholder must
//! never be opened to describe it. Spec: ViewModels/PropertiesViewModel.

use std::os::windows::fs::MetadataExt;

use bb_core::FileTime;
use bb_core::attributes::{DIRECTORY, HIDDEN, READONLY};
use bb_core::columns::MetadataReadRules;
use bb_core::format::{byte_size, thousands};
use serde_json::{Value, json};
use windows_sys::Win32::Storage::FileSystem::SetFileAttributesW;

use crate::app::Inner;

struct Stat {
    attributes: u32,
    created: i64,
    modified: i64,
    accessed: i64,
    length: u64,
}

fn stat(path: &str) -> Option<Stat> {
    // The item itself, not what a link points at: its attributes are the ones a change would write.
    let m = std::fs::symlink_metadata(path).ok()?;
    Some(Stat {
        attributes: m.file_attributes(),
        created: m.creation_time() as i64,
        modified: m.last_write_time() as i64,
        accessed: m.last_access_time() as i64,
        length: m.file_size(),
    })
}

fn name_of(path: &str) -> String {
    match bb_paths::file_name(path.trim_end_matches(['\\', '/'])) {
        "" => path.to_string(),
        n => n.to_string(),
    }
}

fn plural(n: usize, one: &str) -> String {
    format!(
        "{} {one}{}",
        thousands(n as i64),
        if n == 1 { "" } else { "s" }
    )
}

fn with_bytes(bytes: i64) -> String {
    format!("{} ({} bytes)", byte_size(bytes), thousands(bytes))
}

fn extension_of(path: &str) -> String {
    let name = name_of(path);
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => name[i + 1..].to_uppercase(),
        _ => String::new(),
    }
}

fn wide(path: &str) -> Vec<u16> {
    path.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Whether the items agree on a flag: Some(on) when they all do, None when they differ.
fn common(stats: &[(String, Stat)], flag: u32) -> Option<bool> {
    let mut seen: Option<bool> = None;
    for (_, s) in stats {
        let on = s.attributes & flag != 0;
        match seen {
            None => seen = Some(on),
            Some(prev) if prev != on => return None,
            _ => {}
        }
    }
    seen
}

impl Inner {
    /// `{single, title, name, typeName, location, itemNames, attributesHint, missing, error, size,
    /// created, modified, accessed (ticks, single only), readOnly, hidden (true/false/null when the
    /// items differ), folderSize, folderCounts, sizeComputed, detailsPath (the one file whose shell
    /// properties the host may read, else null)}` for these items.
    pub(crate) fn properties_json(&self, paths: &[String], error: Option<String>) -> Value {
        let single = paths.len() == 1;
        let is_dir = |p: &str| std::path::Path::new(p).is_dir();
        let dirs: Vec<&String> = paths.iter().filter(|p| is_dir(p)).collect();
        let files = paths.len() - dirs.len();
        let name = if single {
            name_of(&paths[0])
        } else {
            format!("{} items", thousands(paths.len() as i64))
        };
        let type_name = if single {
            if is_dir(&paths[0]) {
                "Folder".to_string()
            } else {
                match extension_of(&paths[0]) {
                    e if e.is_empty() => "File".to_string(),
                    e => format!("{e} file"),
                }
            }
        } else if files == 0 {
            plural(dirs.len(), "folder")
        } else {
            let mut exts: Vec<String> = paths
                .iter()
                .filter(|p| !is_dir(p))
                .map(|p| extension_of(p))
                .collect();
            exts.sort();
            exts.dedup();
            let files_label = match exts.as_slice() {
                [only] if !only.is_empty() => plural(files, &format!("{only} file")),
                _ => plural(files, "file"),
            };
            if dirs.is_empty() {
                files_label
            } else {
                format!("{files_label}, {}", plural(dirs.len(), "folder"))
            }
        };
        let parent = |p: &str| {
            bb_paths::split_last(p.trim_end_matches(['\\', '/']))
                .0
                .to_string()
        };
        let location = if single {
            paths[0].clone()
        } else {
            let first = parent(&paths[0]);
            if !first.is_empty() && paths.iter().all(|p| parent(p).eq_ignore_ascii_case(&first)) {
                first
            } else {
                "Multiple locations".to_string()
            }
        };

        let stats: Vec<(String, Stat)> = paths
            .iter()
            .filter_map(|p| stat(p).map(|s| (p.clone(), s)))
            .collect();
        let missing = stats.is_empty();
        let error = error.or_else(|| {
            if missing {
                Some(if single {
                    "This item no longer exists on disk.".into()
                } else {
                    "None of the selected items exist on disk any more.".into()
                })
            } else if stats.len() < paths.len() {
                Some(format!(
                    "{} of the selected items no longer exist on disk.",
                    paths.len() - stats.len()
                ))
            } else {
                None
            }
        });

        let (mut size, mut created, mut modified, mut accessed) = (None, None, None, None);
        if single {
            if let Some((_, s)) = stats.first() {
                created = Some(s.created);
                modified = Some(s.modified);
                accessed = Some(s.accessed);
                if s.attributes & DIRECTORY == 0 {
                    size = Some(with_bytes(s.length as i64));
                }
            }
        } else if !missing {
            let bytes: i64 = stats
                .iter()
                .filter(|(_, s)| s.attributes & DIRECTORY == 0)
                .map(|(_, s)| s.length as i64)
                .sum();
            size = Some(
                with_bytes(bytes)
                    + if dirs.is_empty() {
                        ""
                    } else {
                        " \u{2014} files only"
                    },
            );
        }

        // Folder totals come from the MFT pass's cache, all or nothing: a sum over some of them would
        // be a smaller number presented as the answer.
        let (mut folder_size, mut folder_counts, mut size_computed) = (None, None, None);
        if !dirs.is_empty() {
            let cached: Vec<_> = dirs
                .iter()
                .map(|d| self.dir_sizes.get(d).ok().flatten())
                .collect();
            if cached.iter().all(Option::is_some) {
                let results: Vec<_> = cached.into_iter().flatten().collect();
                let bytes: i64 = results.iter().map(|r| r.size_bytes).sum();
                let file_count: i64 = results.iter().map(|r| i64::from(r.file_count)).sum();
                let dir_count: i64 = results.iter().map(|r| i64::from(r.dir_count)).sum();
                let oldest = results
                    .iter()
                    .map(|r| r.computed_utc)
                    .min_by_key(|t| t.0)
                    .unwrap_or(FileTime(0));
                folder_size = Some(with_bytes(bytes));
                folder_counts = Some(format!(
                    "{} files, {} folders",
                    thousands(file_count),
                    thousands(dir_count)
                ));
                let mut computed = format!(
                    "Computed {}",
                    bb_core::format::day(Some(oldest.to_local()), FileTime::now().to_local(), "")
                );
                if results.len() > 1 {
                    computed.push_str(&format!(
                        " \u{2014} total across {} folders",
                        thousands(results.len() as i64)
                    ));
                }
                if results.iter().any(|r| r.incomplete) {
                    computed
                        .push_str(" \u{2014} may be incomplete, some folders were inaccessible");
                }
                size_computed = Some(computed);
            }
        }

        // The shell's own list, for one file the rules let be opened.
        let details_path = (single
            && stats
                .first()
                .is_some_and(|(_, s)| MetadataReadRules::may_read(s.attributes)))
        .then(|| paths[0].clone());

        json!({
            "single": single,
            "title": format!("{name} Properties"),
            "name": name,
            "typeName": type_name,
            "location": location,
            "itemNames": paths.iter().map(|p| name_of(p)).collect::<Vec<_>>(),
            "attributesHint": if single {
                "Applies to this item only, not folder contents.".to_string()
            } else {
                format!("Applies to all {} selected items, not folder contents.", paths.len())
            },
            "missing": missing,
            "hasDirectories": !dirs.is_empty(),
            "error": error,
            "size": size,
            "created": created,
            "modified": modified,
            "accessed": accessed,
            "readOnly": common(&stats, READONLY),
            "hidden": common(&stats, HIDDEN),
            "folderSize": folder_size,
            "folderCounts": folder_counts,
            "sizeComputed": size_computed,
            "detailsPath": details_path,
        })
    }

    /// Sets Read-only and/or Hidden (None leaves a flag alone - what a mixed box the user did not
    /// touch means) on each item, re-reading each first so a bit someone else changed meanwhile, and
    /// every other bit, survives. Answers the model again, with what happened in `error`.
    pub(crate) fn properties_apply(
        self: &std::sync::Arc<Self>,
        paths: &[String],
        read_only: Option<bool>,
        hidden: Option<bool>,
    ) -> Value {
        let mut failures: Vec<String> = Vec::new();
        let mut changed = 0usize;
        for path in paths {
            let Some(s) = stat(path) else {
                failures.push(format!("{}: no longer exists", name_of(path)));
                continue;
            };
            let mut next = s.attributes;
            for (flag, want) in [(READONLY, read_only), (HIDDEN, hidden)] {
                match want {
                    Some(true) => next |= flag,
                    Some(false) => next &= !flag,
                    None => {}
                }
            }
            if next == s.attributes {
                continue;
            }
            // SAFETY: a NUL-terminated path for the duration of the call.
            if unsafe { SetFileAttributesW(wide(path).as_ptr(), next) } != 0 {
                changed += 1;
            } else {
                let e = std::io::Error::last_os_error();
                failures.push(if e.kind() == std::io::ErrorKind::PermissionDenied {
                    format!("{}: access denied", name_of(path))
                } else {
                    format!("{}: {e}", name_of(path))
                });
            }
        }
        let error = match (failures.len(), changed) {
            (0, _) => None,
            (n, c) if c > 0 => Some(format!(
                "Changed {c} item(s); {n} failed \u{2014} {}",
                failures[0]
            )),
            (1, _) => Some(format!(
                "Could not change attributes \u{2014} {}",
                failures[0]
            )),
            (n, _) => Some(format!(
                "Could not change attributes on {n} items \u{2014} {}",
                failures[0]
            )),
        };
        if changed > 0 {
            // The lists showing these folders pick the new attributes up (a hidden file may vanish).
            for (tab, _) in self.all_tabs() {
                self.refresh(tab).ok();
            }
        }
        let mut model = self.properties_json(paths, error);
        model["changed"] = json!(changed);
        model
    }
}
