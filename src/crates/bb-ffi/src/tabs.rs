//! Panes and tabs.

use super::*;

// ---- panes and tabs ----

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BbOrientation {
    /// Children sit side by side.
    BB_VERTICAL = 0,
    /// Children are stacked.
    BB_HORIZONTAL = 1,
}

/// Splits `pane` (0 = the active pane), putting a new pane with one tab at `path` beside it. Returns
/// the new pane.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_pane_split(
    app: *mut BbApp,
    pane: u64,
    orientation: BbOrientation,
    path: BbStr,
    out_pane: *mut u64,
) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let o = match orientation {
            BbOrientation::BB_VERTICAL => SplitOrientation::Vertical,
            BbOrientation::BB_HORIZONTAL => SplitOrientation::Horizontal,
        };
        let pane = if pane == 0 {
            app.app.active_pane()
        } else {
            PaneId(pane)
        };
        let new_pane = app.app.split_pane(pane, o, &path.to_string_lossy())?;
        if let Some(out_pane) = opt_mut(out_pane) {
            *out_pane = new_pane.0;
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_pane_close(app: *mut BbApp, pane: u64) -> BbStatus {
    guard(|| Ok(app_of(app)?.app.close_pane(PaneId(pane))?))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_pane_activate(app: *mut BbApp, pane: u64) -> BbStatus {
    guard(|| Ok(app_of(app)?.app.activate_pane(PaneId(pane))?))
}

/// Opens a tab in `pane` (0 = the active pane). Returns the new tab.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_open(
    app: *mut BbApp,
    pane: u64,
    path: BbStr,
    activate: bool,
    out_tab: *mut u64,
) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let pane = (pane != 0).then_some(PaneId(pane));
        let tab = app.app.open_tab(pane, &path.to_string_lossy(), activate)?;
        if let Some(out_tab) = opt_mut(out_tab) {
            *out_tab = tab.0;
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_close(app: *mut BbApp, tab: u64) -> BbStatus {
    guard(|| Ok(app_of(app)?.app.close_tab(TabId(tab))?))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_activate(app: *mut BbApp, tab: u64) -> BbStatus {
    guard(|| Ok(app_of(app)?.app.activate_tab(TabId(tab))?))
}

/// Navigates to a folder. The listing arrives as `BB_EVT_LISTING_CHANGED`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_navigate(app: *mut BbApp, tab: u64, path: BbStr) -> BbStatus {
    guard(|| {
        Ok(app_of(app)?
            .app
            .navigate(TabId(tab), &path.to_string_lossy())?)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_back(app: *mut BbApp, tab: u64) -> BbStatus {
    guard(|| Ok(app_of(app)?.app.back(TabId(tab))?))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_forward(app: *mut BbApp, tab: u64) -> BbStatus {
    guard(|| Ok(app_of(app)?.app.forward(TabId(tab))?))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_up(app: *mut BbApp, tab: u64) -> BbStatus {
    guard(|| Ok(app_of(app)?.app.up(TabId(tab))?))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_refresh(app: *mut BbApp, tab: u64) -> BbStatus {
    guard(|| Ok(app_of(app)?.app.refresh(TabId(tab))?))
}

/// Sets the folder search box (`global` false) or the whole-PC one (true). Empty text clears it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_set_search(
    app: *mut BbApp,
    tab: u64,
    text: BbStr,
    global: bool,
) -> BbStatus {
    guard(|| {
        Ok(app_of(app)?
            .app
            .set_search(TabId(tab), &text.to_string_lossy(), global)?)
    })
}

/// Turns the flat branch view on or off.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_set_flat(app: *mut BbApp, tab: u64, on: bool) -> BbStatus {
    guard(|| Ok(app_of(app)?.app.set_flat_view(TabId(tab), on)?))
}

/// Lowers how many rows a flat listing shows before it stops and says so (the harness's `flat-cap`);
/// takes effect on the next flat listing. The default is the real ceiling, 50,000.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_flat_set_cap(app: *mut BbApp, cap: u32) -> BbStatus {
    guard(|| Ok(app_of(app)?.app.set_flat_cap(cap as usize)?))
}

/// Sorts by a column id; the same column again flips the direction.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_set_sort(app: *mut BbApp, tab: u64, column: BbStr) -> BbStatus {
    guard(|| {
        Ok(app_of(app)?
            .app
            .set_sort(TabId(tab), &column.to_string_lossy())?)
    })
}

/// The thumbnail slider, 0 (details list) to 1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_set_thumbnail_scale(
    app: *mut BbApp,
    tab: u64,
    scale: f64,
) -> BbStatus {
    guard(|| Ok(app_of(app)?.app.set_thumbnail_scale(TabId(tab), scale)?))
}

/// 0 none, 1 preview, 2 metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_set_side_pane(app: *mut BbApp, tab: u64, pane: u32) -> BbStatus {
    guard(|| {
        Ok(app_of(app)?
            .app
            .set_side_pane(TabId(tab), pane.min(255) as u8)?)
    })
}

/// Enter on a row: a folder opens in place, anything else goes to the launcher.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_open_item(app: *mut BbApp, tab: u64, index: u32) -> BbStatus {
    guard(|| Ok(app_of(app)?.app.open_item(TabId(tab), index)?))
}

/// A tab's scalars. Text lives behind `bb_tab_text`.
#[repr(C)]
pub struct BbTabState {
    pub id: u64,
    pub pane: u64,
    /// Bumped on every new listing snapshot.
    pub version: u64,
    pub count: u32,
    pub selected: u32,
    pub can_back: bool,
    pub can_forward: bool,
    pub can_up: bool,
    /// The rows come from many folders (a search, or a flat view).
    pub is_flattened: bool,
    /// The flattening is a browse of a real folder, not a search result.
    pub is_flat_browse: bool,
    pub is_flat_view: bool,
    pub is_global_search: bool,
    pub sort_descending: bool,
    pub shows_content_matches: bool,
    pub is_search_running: bool,
    /// A reload has started and its rows have not arrived yet.
    pub is_loading: bool,
    /// The error is a locked archive: the banner offers Unlock.
    pub is_archive_locked: bool,
    pub has_error: bool,
    /// 0 none, 1 preview, 2 metadata.
    pub side_pane: u32,
    pub thumbnail_scale: f64,
    /// The preview pane's mode (0 auto, 1 raw, 2 hex) and its two per-tab switches (wrap, 100%).
    pub preview_mode: u32,
    pub preview_wrap: bool,
    pub preview_fit: bool,
    /// Bumped when the core selected rows the view should bring into view (a renamed or new item
    /// landing); `reveal_row` is the first of them, or -1.
    pub reveal_seq: u64,
    pub reveal_row: i64,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_state(
    app: *mut BbApp,
    tab: u64,
    out_state: *mut BbTabState,
) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let out_state = out(out_state, "out_state")?;
        let i = app.app.tab_info(TabId(tab))?;
        *out_state = BbTabState {
            id: i.id.0,
            pane: i.pane.0,
            version: i.version,
            count: i.count as u32,
            selected: i.selected as u32,
            can_back: i.can_back,
            can_forward: i.can_forward,
            can_up: i.can_up,
            is_flattened: i.is_flattened,
            is_flat_browse: i.is_flat_browse,
            is_flat_view: i.is_flat_view,
            is_global_search: i.is_global_search,
            sort_descending: i.sort_desc,
            shows_content_matches: i.shows_content_matches,
            is_search_running: i.is_search_running,
            is_loading: i.is_loading,
            is_archive_locked: i.is_archive_locked,
            has_error: i.error.is_some(),
            side_pane: u32::from(i.side_pane),
            thumbnail_scale: i.thumbnail_scale,
            preview_mode: u32::from(i.preview_mode),
            preview_wrap: i.preview_wrap,
            preview_fit: i.preview_fit,
            reveal_seq: i.reveal_seq,
            reveal_row: i.reveal_row.map_or(-1, |r| r as i64),
        };
        Ok(())
    })
}

/// Which text of a tab to read.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BbTabText {
    BB_TEXT_PATH = 0,
    BB_TEXT_TITLE = 1,
    BB_TEXT_STATUS = 2,
    /// The error message; none (NULL) when the listing did not fail.
    BB_TEXT_ERROR = 3,
    /// The message shown over an empty result; none when there is no such message.
    BB_TEXT_EMPTY = 4,
    /// A note the listing is not saying for itself (a flat view stopped at its ceiling); none if none.
    BB_TEXT_NOTICE = 5,
    /// The text of the active search box.
    BB_TEXT_SEARCH = 6,
    /// The id of the column the list is sorted by.
    BB_TEXT_SORT_BY = 7,
    /// "3 items selected (1.2 MB)"; none with nothing selected.
    BB_TEXT_SELECTION = 8,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_text(
    app: *mut BbApp,
    tab: u64,
    which: BbTabText,
    out_text: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let out_text = out(out_text, "out_text")?;
        if which == BbTabText::BB_TEXT_SELECTION {
            let summary = app.app.selection_summary(TabId(tab))?;
            *out_text = summary.map_or_else(BbOwnedStr::none, |v| BbOwnedStr::from_str(&v));
            return Ok(());
        }
        let i = app.app.tab_info(TabId(tab))?;
        let value: Option<String> = match which {
            BbTabText::BB_TEXT_PATH => Some(i.path),
            BbTabText::BB_TEXT_TITLE => Some(i.title),
            BbTabText::BB_TEXT_STATUS => Some(i.status),
            BbTabText::BB_TEXT_ERROR => i.error,
            BbTabText::BB_TEXT_EMPTY => i.empty_message,
            BbTabText::BB_TEXT_NOTICE => i.notice,
            BbTabText::BB_TEXT_SEARCH => Some(i.search_text),
            BbTabText::BB_TEXT_SORT_BY => Some(i.sort_by),
            BbTabText::BB_TEXT_SELECTION => None,
        };
        *out_text = value.map_or_else(BbOwnedStr::none, |v| BbOwnedStr::from_str(&v));
        Ok(())
    })
}

/// The breadcrumb as lines of `name<TAB>full path`, root first.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_breadcrumb(
    app: *mut BbApp,
    tab: u64,
    out_text: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let out_text = out(out_text, "out_text")?;
        let crumbs = app.app.breadcrumb(TabId(tab))?;
        let text = crumbs
            .iter()
            .map(|c| format!("{}\t{}", c.name, c.full_path))
            .collect::<Vec<_>>()
            .join("\n");
        *out_text = BbOwnedStr::from_str(&text);
        Ok(())
    })
}

/// The columns the tab shows, as lines of
/// `id<TAB>header<TAB>width<TAB>cell index<TAB>right aligned (0/1)<TAB>sortable (0/1)`. The cell
/// index is what `bb_listing_cell` takes, or -1 for a column whose values this build cannot read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_columns(
    app: *mut BbApp,
    tab: u64,
    out_text: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let out_text = out(out_text, "out_text")?;
        let columns = app.app.columns_with_cells(TabId(tab))?;
        let text = columns
            .iter()
            .map(|(c, cell)| {
                format!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    c.spec.id,
                    c.spec.header,
                    c.width,
                    cell,
                    u8::from(c.spec.right_aligned),
                    u8::from(c.spec.sortable)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        *out_text = BbOwnedStr::from_str(&text);
        Ok(())
    })
}
