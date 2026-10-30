//! A tab's columns: what the user arranged, what the layout rules make of it, and the values of the
//! shell-property columns, which the host reads. Spec: FileListViewModel (ColumnLayout,
//! ColumnsCustomized, ApplyDefaultColumns), DirectoryTabView (the column menu and gripper),
//! ShellMetadataHydrator.
//!
//! **The property seam is pull, not callback.** Rust never re-enters the host, so the host asks
//! (`prop_requests`) for the cells it should read and answers (`prop_supply`). While the host has
//! not looked at a fresh listing (`props_pending`) and while it owes answers (`props_inflight`) the
//! tab counts as *hydrating*, which is part of the settle predicate; a host that attaches no
//! provider (`propertyProvider` flag off) leaves every property cell blank and never holds a
//! settle. A blank is unknown, never wrong.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use bb_core::FileTime;
use bb_core::columns::{
    ColumnCandidates, ColumnCatalog, ColumnKind, ColumnLayoutRules, ColumnSetting, ColumnValue,
    ResolvedColumn,
};
use bb_paths::{PathKey, fold_upper};
use serde_json::{Value, json};

use crate::app::{Inner, TabData, lock};
use crate::events::{AppEvent, Model};
use crate::ids::{AppError, AppResult, TabId};
use crate::rows::{CELL_COLUMNS, Listing, Row, RowProps, cell_index};

/// One answered cell.
#[derive(Clone, Debug)]
pub struct PropSupply {
    pub path: String,
    pub canonical: String,
    /// None: the handler had no value for this file.
    pub value: Option<ColumnValue>,
}

impl TabData {
    /// The columns in force: the tab's own, or the catalogue default.
    pub(crate) fn column_layout(&self) -> Option<&[ColumnSetting]> {
        self.columns.as_deref()
    }

    pub(crate) fn resolved_columns(&self) -> Vec<ResolvedColumn> {
        ColumnLayoutRules::resolve(
            self.column_layout(),
            self.is_flattened,
            self.shows_content_matches,
            self.compare.is_some(),
        )
    }

    /// The shell-property columns showing, upper-cased canonical names in column order.
    pub(crate) fn prop_ids(&self) -> Vec<String> {
        self.resolved_columns()
            .iter()
            .filter(|c| !c.injected && c.spec.kind == ColumnKind::ShellProperty)
            .map(|c| fold_upper(&c.spec.id))
            .collect()
    }

    /// A snapshot of `rows` in this tab's current shape.
    pub(crate) fn make_listing(
        &self,
        rows: Vec<Arc<Row>>,
        flattened: bool,
        flat_browse: bool,
    ) -> Arc<Listing> {
        if let Some(compare) = &self.compare {
            compare.stamp(&rows);
            // "Differences only" hides the rows the comparison has nothing to say about, but the
            // snapshot keeps all of them: the next rebuild starts from `source_rows`.
            if compare.differences_only {
                let shown: Vec<Arc<Row>> = rows
                    .iter()
                    .filter(|r| compare.shows_when_filtered(r))
                    .cloned()
                    .collect();
                if shown.len() != rows.len() {
                    return Arc::new(
                        Listing::new(self.id.0, self.version, shown, flattened, flat_browse)
                            .with_props(self.prop_ids())
                            .with_source(rows),
                    );
                }
            }
        }
        Arc::new(
            Listing::new(self.id.0, self.version, rows, flattened, flat_browse)
                .with_props(self.prop_ids()),
        )
    }

    /// Whether the host still has cells to read for this listing.
    fn needs_props(&self, provider: bool) -> bool {
        if !provider {
            return false;
        }
        let ids = self.prop_ids();
        if ids.is_empty() {
            return false;
        }
        self.listing.rows().iter().any(|r| {
            self.props
                .get(&r.key)
                .is_none_or(|m| ids.iter().any(|p| !m.contains_key(p)))
        })
    }
}

impl Inner {
    fn property_provider(&self) -> bool {
        lock(&self.extra.ui).flag("propertyProvider")
    }

    /// The tab's own column list (None: the catalogue default), for the palette.
    pub(crate) fn active_tab_columns(&self) -> Option<Vec<ColumnSetting>> {
        let tab = lock(&self.shell).active_tab_id()?;
        let cell = self.tab(tab).ok()?;
        lock(&cell).columns.clone()
    }

    /// The columns the tab shows, each with the cell index `bb_listing_cell` takes.
    pub(crate) fn tab_columns(&self, tab: TabId) -> AppResult<Vec<(ResolvedColumn, i64)>> {
        let cell = self.tab(tab)?;
        let t = lock(&cell);
        let mut next_prop = CELL_COLUMNS.len() as i64;
        Ok(t.resolved_columns()
            .into_iter()
            .map(|c| {
                let index = if !c.injected && c.spec.kind == ColumnKind::ShellProperty {
                    let i = next_prop;
                    next_prop += 1;
                    i
                } else {
                    cell_index(c.id()).map_or(-1, i64::from)
                };
                (c, index)
            })
            .collect())
    }

    pub(crate) fn columns_json(&self, tab: TabId) -> AppResult<Value> {
        let columns = self.tab_columns(tab)?;
        let cell = self.tab(tab)?;
        let t = lock(&cell);
        Ok(json!({
            "customized": t.columns_customized,
            "columns": columns.iter().map(|(c, index)| json!({
                "id": c.spec.id,
                "header": c.spec.header,
                "width": c.width,
                "cell": index,
                "rightAligned": c.spec.right_aligned,
                "sortable": c.spec.sortable,
                "injected": c.injected,
                "property": c.spec.kind == ColumnKind::ShellProperty,
                "canonical": c.spec.canonical_name(),
            })).collect::<Vec<_>>(),
        }))
    }

    /// Re-lays the tab's listing out after its columns changed: the same rows, a new snapshot that
    /// names the property columns, and a fresh ask of the host for cells it lacks.
    pub(crate) fn columns_changed(&self, tab: TabId) {
        let Ok(cell) = self.tab(tab) else { return };
        let provider = self.property_provider();
        let version = {
            let mut t = lock(&cell);
            t.version += 1;
            let rows = t.listing.source_rows().to_vec();
            let (fl, fb) = (t.listing.is_flattened, t.listing.is_flat_browse);
            let spec = t.sort_spec();
            let mut rows = rows;
            rows.sort_by(|a, b| spec.compare(a, b));
            t.listing = t.make_listing(rows, fl, fb);
            t.props_pending = t.needs_props(provider);
            t.version
        };
        self.hub.emit(AppEvent::ListingChanged { tab, version });
        self.hub.emit(AppEvent::TabChanged { tab });
        self.hub.model(Model::Columns);
        self.refresh_hydrating();
        if provider {
            self.hub.emit(AppEvent::PropertiesNeeded { tab });
        }
    }

    /// Arranged here: the tab keeps it, whatever the saved default does next.
    fn columns_arrange(&self, tab: TabId, layout: Vec<ColumnSetting>) -> AppResult<()> {
        {
            let cell = self.tab(tab)?;
            let mut t = lock(&cell);
            t.columns = Some(layout);
            t.columns_customized = true;
        }
        self.columns_changed(tab);
        Ok(())
    }

    fn tab_layout(&self, tab: TabId) -> AppResult<Option<Vec<ColumnSetting>>> {
        let cell = self.tab(tab)?;
        let layout = lock(&cell).columns.clone();
        Ok(layout)
    }

    pub(crate) fn columns_toggle(&self, tab: TabId, id: &str, on: bool) -> AppResult<()> {
        let layout = ColumnLayoutRules::toggle(self.tab_layout(tab)?.as_deref(), id, on);
        self.columns_arrange(tab, layout)
    }

    pub(crate) fn columns_move(&self, tab: TabId, id: &str, index: usize) -> AppResult<()> {
        let layout = ColumnLayoutRules::move_to(self.tab_layout(tab)?.as_deref(), id, index);
        self.columns_arrange(tab, layout)
    }

    pub(crate) fn columns_width(&self, tab: TabId, id: &str, width: f64) -> AppResult<()> {
        let layout = ColumnLayoutRules::set_width(self.tab_layout(tab)?.as_deref(), id, width);
        self.columns_arrange(tab, layout)
    }

    /// The picker's answer: the whole set of shell properties that should be showing.
    pub(crate) fn columns_apply_picked(&self, tab: TabId, chosen: &[String]) -> AppResult<()> {
        let layout = ColumnLayoutRules::apply_picked(self.tab_layout(tab)?.as_deref(), chosen);
        self.columns_arrange(tab, layout)
    }

    /// A header drag: the saved list rewritten to the order the view now has.
    pub(crate) fn columns_capture_order(&self, tab: TabId, live_ids: &[String]) -> AppResult<()> {
        let layout = ColumnLayoutRules::capture_order(live_ids, self.tab_layout(tab)?.as_deref());
        self.columns_arrange(tab, layout)
    }

    /// Back to the catalogue default. (The setter in the original marks the tab customised even for
    /// null, and the default it shows is the catalogue's, not the saved one.)
    pub(crate) fn columns_reset(&self, tab: TabId) -> AppResult<()> {
        {
            let cell = self.tab(tab)?;
            let mut t = lock(&cell);
            t.columns = None;
            t.columns_customized = true;
        }
        self.columns_changed(tab);
        Ok(())
    }

    /// Makes this tab's arrangement what a new tab starts from. Saved at once.
    pub(crate) fn columns_use_for_new_tabs(&self, tab: TabId) -> AppResult<()> {
        let layout = ColumnLayoutRules::normalize(self.tab_layout(tab)?.as_deref());
        lock(&self.settings).file_list_columns = Some(layout);
        self.save_settings_now()?;
        self.columns_default_changed();
        Ok(())
    }

    /// The Columns page: the default set a new tab starts from, `{columns: [{id, header, width,
    /// removable}]}`. Name is first and cannot be removed. Spec: SettingsViewModel (Columns).
    pub(crate) fn columns_defaults_json(&self) -> Value {
        let saved = lock(&self.settings).file_list_columns.clone();
        let layout = ColumnLayoutRules::normalize(saved.as_deref());
        json!({
            "columns": layout.iter().filter_map(|c| {
                let spec = bb_core::columns::ColumnCatalog::try_get(&c.id)?;
                Some(json!({
                    "id": c.id,
                    "header": spec.header,
                    "width": ColumnLayoutRules::sane_width(c.width, spec.default_width),
                    "removable": !c.id.eq_ignore_ascii_case(bb_core::columns::ColumnCatalog::NAME),
                }))
            }).collect::<Vec<_>>(),
        })
    }

    /// One edit of the default set: `add <id>`, `remove <id>`, `move <id> <index>`, `width <id> <dips>`,
    /// `step-width <id> <notches> [fine]` (the wheel over a width: a snapped step, or one DIP with
    /// Ctrl), `reset`. Through the same rules the header menu and a header drag use, so Name stays first
    /// whichever way it is asked for. Saved at once; tabs that never arranged their own follow.
    pub(crate) fn columns_defaults_do(&self, verb: &str, arg: &str) -> AppResult<()> {
        let saved = lock(&self.settings).file_list_columns.clone();
        let current = saved.as_deref();
        let mut parts = arg.split_whitespace();
        let next = match verb {
            "add" => ColumnLayoutRules::toggle(current, arg.trim(), true),
            "remove" => ColumnLayoutRules::toggle(current, arg.trim(), false),
            "move" => {
                let id = parts.next().unwrap_or("");
                let at = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                ColumnLayoutRules::move_to(current, id, at)
            }
            "width" => {
                let id = parts.next().unwrap_or("");
                let width = parts
                    .next()
                    .and_then(|n| n.parse().ok())
                    .ok_or_else(|| AppError::Invalid("width takes '<id> <dips>'".into()))?;
                ColumnLayoutRules::set_width(current, id, width)
            }
            "step-width" => {
                let id = parts.next().unwrap_or("");
                let notches: i32 = parts
                    .next()
                    .and_then(|n| n.parse().ok())
                    .ok_or_else(|| AppError::Invalid("step-width takes '<id> <notches>'".into()))?;
                let fine = parts.next() == Some("fine");
                let from = ColumnLayoutRules::normalize(current)
                    .iter()
                    .find(|c| c.id.eq_ignore_ascii_case(id))
                    .map(|c| c.width)
                    .ok_or(AppError::Stale)?;
                let width = ColumnLayoutRules::step_width(from, notches, fine);
                ColumnLayoutRules::set_width(current, id, width)
            }
            "reset" => bb_core::columns::ColumnCatalog::defaults(),
            other => return Err(AppError::Invalid(format!("unknown columns verb '{other}'"))),
        };
        lock(&self.settings).file_list_columns = Some(next);
        self.save_settings_now()?;
        self.columns_default_changed();
        self.hub.model(Model::Columns);
        Ok(())
    }

    /// The saved default moved (the button above, or the Columns page): every tab that has not
    /// arranged its own takes it. A re-layout, not a reload.
    pub(crate) fn columns_default_changed(&self) {
        let default = lock(&self.settings).file_list_columns.clone();
        for (id, cell) in self.all_tabs() {
            let moved = {
                let mut t = lock(&cell);
                if t.columns_customized {
                    false
                } else {
                    t.columns = default.clone();
                    true
                }
            };
            if moved {
                self.columns_changed(id);
            }
        }
    }

    /// The header strip's checklist: `{builtIns: [{id, header, on, locked}], groups: [{title, items: [...]}],
    /// extra: [...]}` - every built-in in catalogue order (not Folder or Match, which are not choices),
    /// the curated properties by group, then anything showing that neither names (a property added
    /// through the picker, whose tick here is its only way back off). Spec: ColumnHeaderMenu.Rebuild.
    pub(crate) fn columns_header_menu_json(&self, tab: TabId) -> AppResult<Value> {
        use bb_core::columns::ColumnCatalog;
        let current = ColumnLayoutRules::normalize(self.tab_layout(tab)?.as_deref());
        let on = |id: &str| current.iter().any(|c| c.id.eq_ignore_ascii_case(id));
        let row = |id: &str, header: &str| json!({ "id": id, "header": header, "on": on(id), "locked": id.eq_ignore_ascii_case(ColumnCatalog::NAME) });
        let built_ins: Vec<Value> = ColumnCatalog::built_ins()
            .iter()
            .filter(|s| !ColumnCatalog::is_injected(&s.id))
            .map(|s| row(&s.id, &s.header))
            .collect();
        let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
        for spec in ColumnCatalog::curated() {
            match groups.iter_mut().find(|(g, _)| *g == spec.group) {
                Some((_, items)) => items.push(row(&spec.id, &spec.header)),
                None => groups.push((spec.group.clone(), vec![row(&spec.id, &spec.header)])),
            }
        }
        let named = |id: &str| {
            ColumnCatalog::built_ins()
                .iter()
                .chain(ColumnCatalog::curated())
                .any(|s| s.id.eq_ignore_ascii_case(id))
        };
        let extra: Vec<Value> = current
            .iter()
            .filter(|c| !named(&c.id))
            .filter_map(|c| ColumnCatalog::try_get(&c.id))
            .map(|s| row(&s.id, &s.header))
            .collect();
        Ok(json!({
            "builtIns": built_ins,
            "groups": groups.into_iter().map(|(title, items)| json!({ "title": title, "items": items })).collect::<Vec<_>>(),
            "extra": extra,
        }))
    }

    /// What the Add column popup offers; tab 0 asks about the default set (the Columns page).
    pub(crate) fn columns_candidates_json(&self, tab: TabId, search: &str) -> AppResult<Value> {
        let layout = if tab.0 == 0 {
            lock(&self.settings).file_list_columns.clone()
        } else {
            self.tab_layout(tab)?
        };
        let (machine, loaded) = lock(&self.extra.machine_props).clone();
        let candidates = ColumnCandidates::build(layout.as_deref(), &machine, search, loaded);
        Ok(json!({
            "loading": candidates.is_loading,
            "full": candidates.is_full,
            "empty": candidates.is_empty(),
            "max": bb_core::columns::ColumnLayoutRules::MAX_COLUMNS,
            "groups": candidates.groups.iter().map(|g| json!({
                "title": g.title,
                "items": g.items.iter().map(|i| json!({
                    "id": i.id, "header": i.header, "detail": i.detail,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }))
    }

    /// The properties this machine reported (canonical name, localised display name).
    pub(crate) fn columns_set_machine_properties(&self, props: Vec<(String, String)>) {
        *lock(&self.extra.machine_props) = (props, true);
        self.hub.model(Model::Columns);
    }

    // ---- the property seam ----

    pub(crate) fn refresh_hydrating(&self) {
        let mut total = 0;
        for (_, cell) in self.all_tabs() {
            let t = lock(&cell);
            total += usize::from(t.props_pending) + t.props_inflight.len();
        }
        self.hydrating.store(total, Ordering::SeqCst);
        self.update_busy();
    }

    /// The cells the host should read for rows `from..from+count` (count 0: the first 256): path
    /// and canonical name. Each is handed out once; asking marks the listing as looked at.
    pub(crate) fn prop_requests(
        &self,
        tab: TabId,
        from: usize,
        count: usize,
    ) -> AppResult<Vec<(String, String)>> {
        let cell = self.tab(tab)?;
        let mut out = Vec::new();
        {
            let mut t = lock(&cell);
            let names: Vec<(String, String)> = t
                .resolved_columns()
                .iter()
                .filter(|c| !c.injected && c.spec.kind == ColumnKind::ShellProperty)
                .map(|c| (fold_upper(&c.spec.id), c.spec.id.clone()))
                .collect();
            let count = if count == 0 { 256 } else { count };
            let rows = t.listing.clone();
            for row in rows.rows().iter().skip(from).take(count) {
                let known: HashSet<String> = t
                    .props
                    .get(&row.key)
                    .map(|m| m.keys().cloned().collect())
                    .unwrap_or_default();
                for (folded, canonical) in &names {
                    if known.contains(folded) {
                        continue;
                    }
                    let token = (row.key.clone(), folded.clone());
                    if t.props_inflight.contains(&token) {
                        continue;
                    }
                    t.props_inflight.insert(token);
                    out.push((row.full_path.clone(), canonical.clone()));
                }
            }
            t.props_pending = false;
        }
        self.refresh_hydrating();
        Ok(out)
    }

    /// The host's answers. One new snapshot per batch.
    pub(crate) fn prop_supply(&self, tab: TabId, supplied: Vec<PropSupply>) -> AppResult<()> {
        let cell = self.tab(tab)?;
        let version = {
            let mut t = lock(&cell);
            let mut touched: HashSet<Arc<str>> = HashSet::new();
            for s in supplied {
                let Ok(key) = PathKey::canonicalize(&s.path) else {
                    continue;
                };
                let key: Arc<str> = Arc::from(key);
                let folded = fold_upper(&s.canonical);
                t.props_inflight.remove(&(key.clone(), folded.clone()));
                t.props
                    .entry(key.clone())
                    .or_default()
                    .insert(folded, s.value);
                touched.insert(key);
            }
            let stamps: Vec<(Arc<str>, (i64, i64))> = t
                .listing
                .rows()
                .iter()
                .filter(|r| touched.contains(&r.key))
                .map(|r| (r.key.clone(), (r.modified.0, r.size.unwrap_or(-1))))
                .collect();
            t.prop_stamps.extend(stamps);
            let spec = t.sort_spec();
            let mut rows: Vec<Arc<Row>> = Vec::with_capacity(t.listing.len());
            for row in t.listing.source_rows() {
                if touched.contains(&row.key) {
                    let mut next = row.duplicate();
                    let map: RowProps = t.props.get(&row.key).cloned().unwrap_or_default();
                    next.props = Some(Arc::new(map));
                    rows.push(Arc::new(next));
                } else {
                    rows.push(row.clone());
                }
            }
            if ColumnCatalog::sort_spec(Some(&t.sort_by)).kind == ColumnKind::ShellProperty {
                rows.sort_by(|a, b| spec.compare(a, b));
            }
            t.version += 1;
            let (fl, fb) = (t.listing.is_flattened, t.listing.is_flat_browse);
            t.listing = t.make_listing(rows, fl, fb);
            t.version
        };
        self.hub.emit(AppEvent::ListingChanged { tab, version });
        self.refresh_hydrating();
        Ok(())
    }

    /// A new listing arrived: attach what the host already supplied, and ask for the rest.
    pub(crate) fn props_after_install(&self, tab: TabId) {
        let provider = self.property_provider();
        if let Ok(cell) = self.tab(tab) {
            let mut t = lock(&cell);
            t.props_pending = t.needs_props(provider);
            let pending = t.props_pending;
            drop(t);
            if pending {
                self.hub.emit(AppEvent::PropertiesNeeded { tab });
            }
        }
        self.refresh_hydrating();
    }
}

/// A value for tests and the harness's canned provider.
pub fn words(display: &str) -> ColumnValue {
    ColumnValue::words(display)
}

/// A date value (UTC FILETIME ticks).
pub fn date(display: &str, utc: i64) -> ColumnValue {
    ColumnValue::date(display, FileTime(utc))
}
