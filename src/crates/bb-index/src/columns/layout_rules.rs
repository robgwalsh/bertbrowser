//! Turns a saved column list into the columns a list actually shows, and edits that saved list.
//! Pure — the whole of the thinking behind configurable columns, where tests can reach it.
//!
//! `resolve` is where the mode-driven columns (Folder, Match, Status) are placed: they follow the
//! list's *mode* rather than anyone's choice, so a hidden one is genuinely absent rather than a
//! zero-width header with a gripper in the tab order. Every editing operation returns a new list,
//! and the header menu, the reorder drag and the settings page all go through these functions, so
//! the three cannot disagree about what "move this column" means.
//! Spec: Core/Services/Columns/ColumnLayoutRules.cs.

use std::collections::{HashMap, HashSet};

use bb_paths::fold_upper;

use super::{ColumnCatalog, ColumnKind, ColumnSetting, ResolvedColumn};

pub struct ColumnLayoutRules;

impl ColumnLayoutRules {
    /// A row realizes one cell per column, so this bounds what a hand-edited or corrupt settings
    /// file can cost.
    pub const MAX_COLUMNS: usize = 24;

    pub const MIN_WIDTH: f64 = 40.0;
    pub const MAX_WIDTH: f64 = 2000.0;

    /// How far one notch of the wheel moves a width, and the grid a coarse step snaps to.
    pub const WIDTH_STEP: f64 = 10.0;

    /// A width fit to hand to the view. `NaN` is the case that matters and it is not hypothetical:
    /// double-clicking a column gripper auto-sizes it, which saves `NaN`. Infinity and absurd
    /// numbers come from hand-edited files and from widths saved on a much wider monitor. Degrade
    /// to something usable, never fail.
    pub fn sane_width(width: f64, fallback: f64) -> f64 {
        if width.is_finite() && width > 0.0 {
            return width.clamp(Self::MIN_WIDTH, Self::MAX_WIDTH);
        }
        let safe = if fallback.is_finite() && fallback > 0.0 {
            fallback
        } else {
            120.0
        };
        safe.clamp(Self::MIN_WIDTH, Self::MAX_WIDTH)
    }

    /// A width nudged by `notches` turns of the wheel.
    ///
    /// A coarse step **snaps onto the grid** rather than adding to whatever arbitrary number a
    /// dragged gripper left behind: from 137, up is 140. Otherwise spinning the wheel would carry
    /// that 7 along forever. Fine steps do not snap, because their whole purpose is reaching the
    /// number between the grid lines. Everything lands through [`Self::sane_width`], so a `NaN`
    /// from an auto-sized column is repaired here rather than becoming `NaN + 10`.
    pub fn step_width(width: f64, notches: i32, fine: bool) -> f64 {
        let from = Self::sane_width(width, 120.0);
        if notches == 0 {
            return from;
        }
        let notches = f64::from(notches);
        if fine {
            return Self::sane_width(from + notches, from);
        }

        // Snap first, then step, so the notch is never spent only on the snap: 137 up is 140 and
        // 137 down is 130, both one visible move.
        let grid = if notches > 0.0 {
            (from / Self::WIDTH_STEP).floor() * Self::WIDTH_STEP
        } else {
            (from / Self::WIDTH_STEP).ceil() * Self::WIDTH_STEP
        };
        Self::sane_width(grid + notches * Self::WIDTH_STEP, from)
    }

    /// The columns to show: the saved list, cleaned up, with the mode-driven ones put in place.
    ///
    /// `user` is what was saved. **None means never configured** and ships
    /// [`ColumnCatalog::defaults`]; an **empty list is honoured as empty**, because the Name rule
    /// in [`Self::normalize`] means an empty layout is still a usable one, and one rule beats two.
    /// `is_comparing` says this list is one side of a folder comparison, so every row has a status
    /// worth naming in words as well as tinting.
    pub fn resolve(
        user: Option<&[ColumnSetting]>,
        is_flattened: bool,
        shows_content_matches: bool,
        is_comparing: bool,
    ) -> Vec<ResolvedColumn> {
        let settings = Self::normalize(user);
        let mut resolved = Vec::with_capacity(settings.len() + 3);

        for setting in &settings {
            // normalize dropped anything without a spec.
            let spec = ColumnCatalog::try_get(&setting.id).expect("normalized id has a spec");
            let width = Self::sane_width(setting.width, spec.default_width);
            resolved.push(ResolvedColumn {
                spec,
                width,
                injected: false,
            });
        }

        // Folder sits between the name and everything else, and only in a flattened result: in a
        // directory listing every row has the same folder, the one you are in.
        let mut at = 1;
        if is_flattened {
            resolved.insert(
                at.min(resolved.len()),
                injected(ColumnCatalog::RELATIVE_PATH),
            );
            at += 1;
        }

        // And Match only when the search actually read file contents — keyed on that and never on
        // is_flattened, which every search sets. An empty column appearing whenever you type into
        // the box would read as a rendering fault.
        if shows_content_matches {
            resolved.insert(at.min(resolved.len()), injected(ColumnCatalog::MATCH));
        }

        // Status goes at the end rather than beside the name: it is the answer to a question about
        // the row, not part of what the row is, and putting it first would push the columns someone
        // arranged for this folder along by one every time a comparison started.
        if is_comparing {
            resolved.push(injected(ColumnCatalog::COMPARE_STATUS));
        }

        resolved
    }

    /// The saved list, made usable: unrenderable ids dropped, duplicates collapsed, injected ids
    /// removed, Name first, and no more than [`Self::MAX_COLUMNS`] of them.
    pub fn normalize(user: Option<&[ColumnSetting]>) -> Vec<ColumnSetting> {
        let defaults;
        let source: &[ColumnSetting] = match user {
            Some(user) => user,
            None => {
                defaults = ColumnCatalog::defaults();
                &defaults
            }
        };
        let mut kept: Vec<ColumnSetting> = Vec::with_capacity(source.len().min(Self::MAX_COLUMNS));
        let mut seen = HashSet::new();

        for setting in source {
            let id = setting.id.trim();

            // Folder and Match are placed by resolve, never chosen. One arriving from a hand-edited
            // file would otherwise appear twice, or appear in a listing where it means nothing.
            if id.is_empty() || ColumnCatalog::is_injected(id) {
                continue;
            }
            let Some(spec) = ColumnCatalog::try_get(id) else {
                continue;
            };
            if !seen.insert(fold_upper(&spec.id)) {
                continue;
            }

            kept.push(ColumnSetting::new(
                &spec.id,
                Self::sane_width(setting.width, spec.default_width),
            ));
            if kept.len() == Self::MAX_COLUMNS {
                break;
            }
        }

        // Name carries the icon and is what a row is identified by; a list without it, or one with
        // it scrolled off the left of a horizontally scrolling view, is unusable. Explorer does not
        // let you move Name either. Enforced here rather than only on load, because the reorder
        // drag comes back through normalize too.
        match index_of(&kept, ColumnCatalog::NAME) {
            Some(index) if index > 0 => {
                let name = kept.remove(index);
                kept.insert(0, name);
            }
            Some(_) => {}
            None => {
                let spec = ColumnCatalog::try_get(ColumnCatalog::NAME).expect("Name is built in");
                if kept.len() == Self::MAX_COLUMNS {
                    kept.pop();
                }
                kept.insert(0, ColumnSetting::new(&spec.id, spec.default_width));
            }
        }

        kept
    }

    /// Adds a column at the end, or removes it. Name cannot be removed.
    pub fn toggle(user: Option<&[ColumnSetting]>, id: &str, on: bool) -> Vec<ColumnSetting> {
        let mut kept = Self::normalize(user);
        let index = index_of(&kept, id);

        if !on {
            // Index 0 is Name, which normalize would only put straight back.
            if let Some(index) = index
                && index > 0
            {
                kept.remove(index);
            }
            return Self::normalize(Some(&kept));
        }

        if index.is_some() {
            return Self::normalize(Some(&kept));
        }
        let Some(spec) = ColumnCatalog::try_get(id) else {
            return Self::normalize(Some(&kept));
        };
        kept.push(ColumnSetting::new(&spec.id, spec.default_width));
        Self::normalize(Some(&kept))
    }

    /// The layout after a trip through the "More columns…" picker, whose answer is the whole set of
    /// shell properties that should be showing.
    ///
    /// Removals first and then additions, both through [`Self::toggle`], so a column that was
    /// already there **keeps its place and its width** rather than being taken out and put back at
    /// the end — opening the picker and pressing OK changes nothing at all. Built-in columns are
    /// not the picker's business and are left exactly as they are: a built-in missing from `chosen`
    /// means "not offered here", never "remove it".
    pub fn apply_picked<S: AsRef<str>>(
        user: Option<&[ColumnSetting]>,
        chosen: &[S],
    ) -> Vec<ColumnSetting> {
        let wanted: HashSet<String> = chosen.iter().map(|c| fold_upper(c.as_ref())).collect();
        let mut layout = Self::normalize(user);

        for setting in layout.clone() {
            let is_property = ColumnCatalog::try_get(&setting.id)
                .is_some_and(|spec| spec.kind == ColumnKind::ShellProperty);
            if is_property && !wanted.contains(&fold_upper(&setting.id)) {
                layout = Self::toggle(Some(&layout), &setting.id, false);
            }
        }

        for id in chosen {
            layout = Self::toggle(Some(&layout), id.as_ref(), true);
        }

        layout
    }

    /// Moves a column to a position. An out-of-range target clamps rather than panics.
    pub fn move_to(
        user: Option<&[ColumnSetting]>,
        id: &str,
        target_index: usize,
    ) -> Vec<ColumnSetting> {
        let mut kept = Self::normalize(user);
        let Some(index) = index_of(&kept, id) else {
            return Self::normalize(Some(&kept));
        };

        let target = target_index.min(kept.len().saturating_sub(1));
        if target == index {
            return Self::normalize(Some(&kept));
        }

        let moved = kept.remove(index);
        kept.insert(target, moved);
        Self::normalize(Some(&kept))
    }

    pub fn set_width(user: Option<&[ColumnSetting]>, id: &str, width: f64) -> Vec<ColumnSetting> {
        let mut kept = Self::normalize(user);
        let Some(index) = index_of(&kept, id) else {
            return kept;
        };

        let spec = ColumnCatalog::try_get(id).expect("a normalized id has a spec");
        kept[index].width = Self::sane_width(width, spec.default_width);
        kept
    }

    /// The saved list rewritten to match the order the view now has, after a header was dragged.
    ///
    /// The live collection holds the injected columns as well, which the model must never learn
    /// about — so they are dropped here rather than at the call site, and widths are carried across
    /// from the settings the ids came from. Name is put back at the front, which is what visually
    /// snaps a dragged Name column home.
    pub fn capture_order<S: AsRef<str>>(
        live_ids: &[S],
        user: Option<&[ColumnSetting]>,
    ) -> Vec<ColumnSetting> {
        let widths: HashMap<String, f64> = Self::normalize(user)
            .into_iter()
            .map(|s| (fold_upper(&s.id), s.width))
            .collect();

        let mut rebuilt = Vec::with_capacity(live_ids.len());
        for id in live_ids {
            let id = id.as_ref();
            if ColumnCatalog::is_injected(id) {
                continue;
            }
            let Some(spec) = ColumnCatalog::try_get(id) else {
                continue;
            };
            let width = widths
                .get(&fold_upper(&spec.id))
                .copied()
                .unwrap_or(spec.default_width);
            rebuilt.push(ColumnSetting::new(&spec.id, width));
        }
        Self::normalize(Some(&rebuilt))
    }
}

fn injected(id: &str) -> ResolvedColumn {
    let spec = ColumnCatalog::try_get(id).expect("an injected id is built in");
    let width = spec.default_width;
    ResolvedColumn {
        spec,
        width,
        injected: true,
    }
}

fn index_of(settings: &[ColumnSetting], id: &str) -> Option<usize> {
    let key = fold_upper(id);
    settings.iter().position(|c| fold_upper(&c.id) == key)
}
