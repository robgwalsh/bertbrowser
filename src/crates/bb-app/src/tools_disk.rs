//! The disk-usage view's model: what a folder is made of, read from the search index and
//! `dir_size_cache`, and the squarified treemap of it. Spec: ViewModels/DiskUsageViewModel.cs,
//! DiskUsageTileViewModel.cs, Views/TreemapCanvas.cs.
//!
//! **A folder whose size is not known reads "Not measured", never "0 B"**, and gets no tile and an
//! empty bar: an unknown size has no share to draw. Directory sizes are never computed here; they
//! come from the cache the MFT pass fills, and a missing row means unknown.

use std::sync::Arc;

use bb_core::cancel::CancelToken;
use bb_core::data::{DirSizeRepository, FsIndexRepository};
use bb_core::diskusage::{
    DiskUsageAvailability, DiskUsageIndex, DiskUsageNode, DiskUsageService, TreemapLayout,
    TreemapPalette,
};
use bb_core::format::byte_size;
use bb_core::fs::LocalFileSystem;
use bb_core::mft::MftIndex;
use bb_core::theming::{ResolvedTheme, ThemeToken};
use serde_json::{Value, json};

use crate::app::Inner;
use crate::ids::{AppError, AppResult};

/// How many of the biggest files are listed.
const LARGEST_FILES_LIMIT: usize = 500;
/// A treemap draws at most this many tiles; the rest are folded into one.
pub(crate) const MAX_TILES: usize = 300;

struct Facts(Arc<dyn MftIndex>);

impl DiskUsageIndex for Facts {
    fn is_building(&self) -> bool {
        self.0.is_building()
    }
    fn is_indexed(&self, path_key: &str) -> bool {
        self.0.is_indexed(path_key)
    }
    fn any_indexed(&self) -> bool {
        self.0.any_indexed()
    }
}

fn message(availability: DiskUsageAvailability, status: &str) -> String {
    // Both of the "not indexed" wordings say what is missing rather than "no sizes": the files
    // listed beside the banner have real sizes, because those come from reading the folder.
    match availability {
        DiskUsageAvailability::Ready => "",
        DiskUsageAvailability::Building if status.is_empty() => "Still indexing",
        DiskUsageAvailability::Building => status,
        DiskUsageAvailability::NoSizeData => {
            "This drive is indexed by name only \u{2014} nothing measured it, so folder totals aren't available."
        }
        DiskUsageAvailability::NotIndexed => {
            "This drive isn't indexed, so folder totals aren't available. File sizes here are real."
        }
        DiskUsageAvailability::NotAPath => "That isn't a folder this can measure.",
    }
    .to_string()
}

fn size_text(node: &DiskUsageNode) -> String {
    match node.size_bytes {
        Some(b) => format!(
            "{}{}",
            byte_size(b),
            if node.incomplete { " *" } else { "" }
        ),
        None => "Not measured".to_string(),
    }
}

fn argb(c: bb_core::theming::ThemeColor) -> u32 {
    u32::from(c.a) << 24 | u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
}

/// The folder disk usage is reading or last read. `seq` tells a superseded read from the current one.
#[derive(Default)]
pub(crate) struct DiskLoad {
    seq: u64,
    path: String,
    loading: bool,
    model: Option<Value>,
    error: Option<String>,
}

impl Inner {
    /// Reads `path` on a worker; what was shown stays until the new answer arrives. Spec:
    /// DiskUsageViewModel.LoadAsync (superseding, IsLoading).
    pub(crate) fn disk_usage_load(self: &Arc<Self>, path: String) {
        let seq = {
            let mut d = crate::app::lock(&self.extra.tools.disk);
            d.seq += 1;
            d.path.clone_from(&path);
            d.loading = true;
            d.seq
        };
        self.hub.model(crate::events::Model::DiskUsage);
        self.schedule(move |inner| {
            let result = inner.disk_usage_model(&path);
            {
                let mut d = crate::app::lock(&inner.extra.tools.disk);
                if d.seq != seq {
                    return;
                }
                d.loading = false;
                match result {
                    Ok(m) => {
                        d.model = Some(m);
                        d.error = None;
                    }
                    Err(e) => {
                        d.model = None;
                        d.error = Some(e.to_string());
                    }
                }
            }
            inner.hub.model(crate::events::Model::DiskUsage);
        });
    }

    pub(crate) fn disk_usage_state(&self) -> Value {
        let d = crate::app::lock(&self.extra.tools.disk);
        json!({ "path": d.path, "loading": d.loading, "model": d.model, "error": d.error })
    }

    /// `{root, rootDisplay, canGoUp, total, unknownChildCount, availability, message, canRetry,
    /// children:[{name, path, isDirectory, size, sizeText, unknown, fraction, synthetic}],
    /// largest:[{name, relativePath, path, sizeText}], largestEmpty, fills, label}` for `root`
    /// (empty for the whole PC).
    pub(crate) fn disk_usage_model(&self, root: &str) -> AppResult<Value> {
        let hidden = crate::app::lock(&self.settings).show_hidden_items;
        let service = DiskUsageService::new(
            Arc::new(FsIndexRepository::new(self.db.clone())),
            Arc::new(DirSizeRepository::new(self.db.clone())),
            Arc::new(LocalFileSystem),
            Arc::new(Facts(self.seams.index.clone())),
            None,
        );
        let cancel = CancelToken::new();
        let status = self.seams.index.status_text();
        let root_opt = (!root.is_empty()).then_some(root);

        let mut availability;
        let mut largest: Vec<Value> = Vec::new();
        match service.largest_files(root_opt, LARGEST_FILES_LIMIT, hidden, &cancel) {
            Ok(files) => {
                availability = files.availability;
                largest = files
                    .files
                    .iter()
                    .map(|f| {
                        json!({
                            "name": f.name,
                            "relativePath": f.relative_dir_display,
                            "path": f.display_path,
                            "sizeText": byte_size(f.size_bytes),
                        })
                    })
                    .collect();
            }
            Err(_) => availability = DiskUsageAvailability::NotAPath,
        }

        let mut children: Vec<Value> = Vec::new();
        let mut total: Option<i64> = None;
        let mut unknown = 0usize;
        let has_children = root_opt.is_some_and(|d| std::path::Path::new(d).is_dir());
        if let (Some(dir), true) = (root_opt, has_children) {
            match service.breakdown(dir, hidden, &cancel) {
                Ok(b) => {
                    total = b.total_bytes;
                    unknown = b.unknown_child_count;
                    let top = b
                        .children
                        .iter()
                        .map(|c| c.size_bytes.unwrap_or(0))
                        .max()
                        .unwrap_or(0);
                    let tile = |c: &DiskUsageNode, synthetic: bool| {
                        json!({
                            "name": c.name,
                            "path": c.display_path,
                            "isDirectory": c.is_directory,
                            "size": c.size_bytes,
                            "sizeText": size_text(c),
                            "unknown": c.size_bytes.is_none(),
                            // Scaled to the largest child, so a folder whose own total is unknown
                            // still gets a readable comparison between its children.
                            "fraction": match c.size_bytes {
                                Some(s) if top > 0 => (s as f64 / top as f64).clamp(0.0, 1.0),
                                _ => 0.0,
                            },
                            "synthetic": synthetic,
                        })
                    };
                    children = b.children.iter().map(|c| tile(c, false)).collect();
                    if let Some(rest) = b.unaccounted_bytes.filter(|u| *u > 0) {
                        let node = DiskUsageNode {
                            path_key: String::new(),
                            display_path: b.root_display_path.clone(),
                            name: "Other files in this folder".into(),
                            is_directory: false,
                            size_bytes: Some(rest),
                            incomplete: false,
                            hidden: false,
                        };
                        children.push(tile(&node, true));
                    }
                    // The breakdown weighs different evidence and is the more specific answer for a
                    // folder, so it wins where the two disagree.
                    availability = b.availability;
                }
                Err(_) => availability = DiskUsageAvailability::NotAPath,
            }
        }

        let text = message(availability, &status);
        let largest_empty = if !largest.is_empty() {
            ""
        } else if availability == DiskUsageAvailability::Ready {
            "Nothing below this folder."
        } else {
            "Needs the search index \u{2014} see above."
        };
        let theme: ResolvedTheme = crate::app::lock(&self.extra.theme).resolved();
        let ramp = TreemapPalette::ramp(&theme, 12);
        let can_go_up = root_opt.is_some_and(|p| crate::tabs::parent_path(p).is_some());
        Ok(json!({
            "root": root,
            "rootDisplay": if root.is_empty() { "This PC" } else { root },
            "canGoUp": can_go_up,
            "total": total,
            "totalText": total.map(byte_size).unwrap_or_default(),
            "unknownChildCount": unknown,
            "availability": format!("{availability:?}"),
            "hasMessage": availability != DiskUsageAvailability::Ready,
            "message": text,
            "canRetry": availability == DiskUsageAvailability::NotIndexed && self.seams.index.can_start(),
            "children": children,
            "largest": largest,
            "largestEmpty": largest_empty,
            "maxTiles": MAX_TILES,
            "fills": ramp.iter().map(|c| argb(*c)).collect::<Vec<_>>(),
            "label": argb(theme[ThemeToken::TextOnAccent]),
            "separator": argb(theme[ThemeToken::BorderSubtle]),
        }))
    }
}

/// The squarified layout of `weights` into `width` x `height`:
/// `[{index, x, y, width, height}]`, largest first, one per positive weight.
pub(crate) fn treemap_layout(weights: &[f64], width: f64, height: f64) -> Value {
    Value::Array(
        TreemapLayout::arrange(weights, width, height)
            .iter()
            .map(|r| json!({"index": r.index, "x": r.x, "y": r.y, "width": r.width, "height": r.height}))
            .collect(),
    )
}

pub(crate) fn require_dir(path: &str) -> AppResult<()> {
    if path.is_empty() || std::path::Path::new(path).exists() {
        Ok(())
    } else {
        Err(AppError::Invalid(format!(
            "There is no folder at '{path}'."
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_gives_every_positive_weight_a_rectangle_and_nothing_else() {
        let v = treemap_layout(&[10.0, 0.0, 5.0, f64::NAN, 1.0], 100.0, 50.0);
        let rects = v.as_array().unwrap();
        assert_eq!(rects.len(), 3);
        let area: f64 = rects
            .iter()
            .map(|r| r["width"].as_f64().unwrap() * r["height"].as_f64().unwrap())
            .sum();
        assert!((area - 5000.0).abs() < 1.0, "area was {area}");
        assert_eq!(rects[0]["index"], 0);
    }

    #[test]
    fn an_unknown_size_reads_not_measured_and_never_zero() {
        let node = DiskUsageNode {
            path_key: "C:\\X".into(),
            display_path: "C:\\x".into(),
            name: "x".into(),
            is_directory: true,
            size_bytes: None,
            incomplete: false,
            hidden: false,
        };
        assert_eq!(size_text(&node), "Not measured");
        let known = DiskUsageNode {
            size_bytes: Some(0),
            ..node.clone()
        };
        assert_eq!(size_text(&known), "0 B");
        let partial = DiskUsageNode {
            size_bytes: Some(2048),
            incomplete: true,
            ..node
        };
        assert_eq!(size_text(&partial), "2 KB *");
    }

    #[test]
    fn each_availability_has_its_own_wording() {
        let all = [
            DiskUsageAvailability::NoSizeData,
            DiskUsageAvailability::NotIndexed,
            DiskUsageAvailability::NotAPath,
        ];
        let words: std::collections::HashSet<String> =
            all.iter().map(|a| message(*a, "")).collect();
        assert_eq!(words.len(), 3);
        assert_eq!(message(DiskUsageAvailability::Ready, ""), "");
        assert_eq!(
            message(DiskUsageAvailability::Building, ""),
            "Still indexing"
        );
    }
}
