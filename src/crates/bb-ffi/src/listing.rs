//! The listing snapshot, its rows and selection.

use super::*;

// ---- the listing ----

/// An immutable, reference-counted snapshot of a tab's rows in display order. Every view it hands out
/// stays valid until the last reference is released.
pub struct BbListing {
    _opaque: (),
}

pub(crate) fn listing_of<'a>(l: *const BbListing) -> Result<&'a Listing, Fail> {
    // SAFETY: a non-null `BbListing*` is a pointer `bb_tab_listing` made from `Arc::into_raw`, and the
    // caller holds a reference until `bb_listing_release`, so the `Listing` behind it is alive.
    unsafe { (l as *const Listing).as_ref() }.ok_or_else(|| Fail::Err("null listing".into()))
}

/// The tab's current snapshot, with one reference for the caller (release it with
/// `bb_listing_release`). NULL when the tab is gone.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_listing(app: *mut BbApp, tab: u64) -> *const BbListing {
    guard_value(ptr::null(), || {
        let Some(app) = opt_ref(app) else {
            return ptr::null();
        };
        match app.app.listing(TabId(tab)) {
            Ok(listing) => Arc::into_raw(listing) as *const BbListing,
            Err(e) => {
                set_last_error(&e.to_string());
                ptr::null()
            }
        }
    })
}

/// Takes another reference to a snapshot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_listing_retain(l: *const BbListing) {
    if l.is_null() {
        return;
    }
    // SAFETY: `l` came from `Arc::into_raw` in `bb_tab_listing` and the caller still holds a reference.
    unsafe { Arc::increment_strong_count(l as *const Listing) };
}

/// Gives a reference back. The snapshot is freed with the last one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_listing_release(l: *const BbListing) {
    if l.is_null() {
        return;
    }
    // SAFETY: `l` came from `Arc::into_raw` and this gives back the reference the caller holds.
    drop(unsafe { Arc::from_raw(l as *const Listing) });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_listing_len(l: *const BbListing) -> u32 {
    guard_value(0, || listing_of(l).map(|l| l.len() as u32).unwrap_or(0))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_listing_version(l: *const BbListing) -> u64 {
    guard_value(0, || listing_of(l).map(|l| l.version).unwrap_or(0))
}

/// `BbRow.flags` bits.
pub const BB_ROW_DIRECTORY: u32 = 1;
pub const BB_ROW_HIDDEN: u32 = 2;
pub const BB_ROW_MEDIA: u32 = 4;
/// A folder size that is a floor, because some folders could not be read.
pub const BB_ROW_SIZE_INCOMPLETE: u32 = 8;
/// Bits 4..6 are what a folder comparison says about the row (`compare_view::state_number`: 0 nothing,
/// 1 not compared, 2 same, 3 only here, 4 newer, 5 older, 6 differs): `(flags >> 4) & 7`.
pub const BB_ROW_COMPARE_SHIFT: u32 = 4;

/// One row, as views into the snapshot. Ask for visible rows at paint time only.
#[repr(C)]
pub struct BbRow {
    /// Stable per path within a tab's browsing of one place, so hover survives a watcher merge.
    pub row_id: u64,
    pub name: BbStr,
    pub full_path: BbStr,
    /// The folder a search or flat result came from, relative to where it was searched; empty
    /// otherwise.
    pub relative_dir: BbStr,
    /// Bytes, or -1 for a folder whose size is not known (blank, never zero).
    pub size: i64,
    /// FILETIME ticks (100 ns since 1601 UTC); 0 when unknown.
    pub modified: i64,
    pub created: i64,
    pub accessed: i64,
    /// The Win32 `FILE_ATTRIBUTE_*` mask.
    pub attributes: u32,
    /// `BB_ROW_*` bits.
    pub flags: u32,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_listing_row(
    l: *const BbListing,
    index: u32,
    out_row: *mut BbRow,
) -> BbStatus {
    guard(|| {
        let listing = listing_of(l)?;
        let out_row = out(out_row, "out_row")?;
        let row = listing
            .row(index as usize)
            .ok_or_else(|| Fail::Err(format!("row {index} is out of range")))?;
        *out_row = BbRow {
            row_id: row.id,
            name: BbStr::of(row.name_wide()),
            full_path: BbStr::of(row.path_wide()),
            relative_dir: BbStr::of(row.relative_dir_wide()),
            size: row.size.unwrap_or(-1),
            modified: row.modified.0,
            created: row.created.0,
            accessed: row.accessed.0,
            attributes: row.attributes,
            flags: row_flags(row),
        };
        Ok(())
    })
}

/// A row's id and `BB_ROW_*` flags, without its text. For questions asked of every row (laying
/// tiles out, finding a row by id): `bb_listing_row` makes the row's UTF-16 strings, which a row
/// otherwise has only once it is painted.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_listing_row_meta(
    l: *const BbListing,
    index: u32,
    out_id: *mut u64,
    out_flags: *mut u32,
) -> BbStatus {
    guard(|| {
        let listing = listing_of(l)?;
        let row = listing
            .row(index as usize)
            .ok_or_else(|| Fail::Err(format!("row {index} is out of range")))?;
        set_opt(out_id, row.id);
        set_opt(out_flags, row_flags(row));
        Ok(())
    })
}

fn row_flags(row: &bb_app::Row) -> u32 {
    let mut flags = 0;
    if row.is_directory {
        flags |= BB_ROW_DIRECTORY;
    }
    if row.is_hidden() {
        flags |= BB_ROW_HIDDEN;
    }
    if row.is_media {
        flags |= BB_ROW_MEDIA;
    }
    if row.size_incomplete {
        flags |= BB_ROW_SIZE_INCOMPLETE;
    }
    flags
        | (u32::from(row.compare.load(std::sync::atomic::Ordering::Relaxed)) & 7)
            << BB_ROW_COMPARE_SHIFT
}

/// The formatted text of one cell. `column` is a cell index from `bb_tab_columns`. The view is valid
/// as long as the snapshot is.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_listing_cell(
    l: *const BbListing,
    index: u32,
    column: u32,
    out_text: *mut BbStr,
) -> BbStatus {
    guard(|| {
        let listing = listing_of(l)?;
        let out_text = out(out_text, "out_text")?;
        let cell = listing
            .cell(index as usize, column)
            .ok_or_else(|| Fail::Err(format!("row {index} is out of range")))?;
        *out_text = BbStr::of(cell);
        Ok(())
    })
}

/// The Size cell's tooltip for a row (see `Row::size_tip`), Rust-owned (`bb_str_free`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_listing_size_tip(
    l: *const BbListing,
    index: u32,
    out_text: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let listing = listing_of(l)?;
        let row = listing
            .row(index as usize)
            .ok_or_else(|| Fail::Err(format!("row {index} is out of range")))?;
        *out(out_text, "out_text")? = BbOwnedStr::from_str(&row.size_tip());
        Ok(())
    })
}

/// The cell index (`bb_listing_cell`) of a built-in column by its id ("Size", "Modified"), or -1:
/// for a view that shows a column the layout does not (a thumbnail view's rows).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_cell_index(id: BbStr, out_index: *mut i32) -> BbStatus {
    guard(|| {
        let out_index = out(out_index, "out_index")?;
        *out_index = bb_app::cell_index(&id.to_string_lossy()).map_or(-1, |i| i as i32);
        Ok(())
    })
}

/// The line a `content:` search matched on, for the Match cell: its 1-based number (0 when the row
/// has no match) and where the needle sits in the cell's text, in UTF-16 units.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_listing_match(
    l: *const BbListing,
    index: u32,
    out_line: *mut i32,
    out_start: *mut u32,
    out_length: *mut u32,
) -> BbStatus {
    guard(|| {
        let listing = listing_of(l)?;
        let row = listing
            .row(index as usize)
            .ok_or_else(|| Fail::Err(format!("row {index} is out of range")))?;
        let (line, start, length) = match row.content_match() {
            Some(m) => {
                // The core counts characters; the cell is drawn in UTF-16.
                let units = |chars: usize| -> u32 {
                    m.line
                        .chars()
                        .take(chars)
                        .map(|c| c.len_utf16() as u32)
                        .sum()
                };
                let start = units(m.match_start);
                (
                    m.line_number,
                    start,
                    units(m.match_start + m.match_length) - start,
                )
            }
            None => (0, 0, 0),
        };
        set_opt(out_line, line);
        set_opt(out_start, start);
        set_opt(out_length, length);
        Ok(())
    })
}

// ---- selection ----

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BbSelectOp {
    /// Select exactly these rows.
    BB_SELECT_REPLACE = 0,
    BB_SELECT_ADD = 1,
    BB_SELECT_TOGGLE = 2,
    BB_SELECT_REMOVE = 3,
    BB_SELECT_ALL = 4,
    BB_SELECT_CLEAR = 5,
    BB_SELECT_INVERT = 6,
}

/// One gesture, one call: `idx` are positions in the snapshot the caller holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_select(
    app: *mut BbApp,
    tab: u64,
    op: BbSelectOp,
    idx: *const u32,
    n: u32,
) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let op = match op {
            BbSelectOp::BB_SELECT_REPLACE => SelectOp::Replace,
            BbSelectOp::BB_SELECT_ADD => SelectOp::Add,
            BbSelectOp::BB_SELECT_TOGGLE => SelectOp::Toggle,
            BbSelectOp::BB_SELECT_REMOVE => SelectOp::Remove,
            BbSelectOp::BB_SELECT_ALL => SelectOp::All,
            BbSelectOp::BB_SELECT_CLEAR => SelectOp::Clear,
            BbSelectOp::BB_SELECT_INVERT => SelectOp::Invert,
        };
        Ok(app.app.select(TabId(tab), op, slice_of(idx, n as usize))?)
    })
}

/// The selected rows' positions in the current snapshot, ascending. Writes up to `cap` of them and
/// reports the total in `out_count`, so a caller can size a second call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_selection(
    app: *mut BbApp,
    tab: u64,
    out_idx: *mut u32,
    cap: u32,
    out_count: *mut u32,
) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let selected = app.app.selected_indices(TabId(tab))?;
        copy_to(&selected[..selected.len().min(cap as usize)], out_idx);
        set_opt(out_count, selected.len() as u32);
        Ok(())
    })
}

/// The selected rows' full paths, one per line.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_selection_paths(
    app: *mut BbApp,
    tab: u64,
    out_text: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        let out_text = out(out_text, "out_text")?;
        *out_text = BbOwnedStr::from_str(&app.app.selected_paths(TabId(tab))?.join("\n"));
        Ok(())
    })
}
