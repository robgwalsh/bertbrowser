//! One row of a listing, and the immutable snapshot the UI paints from. Spec: FileItemViewModel,
//! the display half. A snapshot is never mutated: every load, merge or re-sort builds a new one and
//! announces it with `ListingChanged`, so a painting thread never sees a half-applied change.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use bb_core::FileTime;
use bb_core::attributes::{
    ARCHIVE, COMPRESSED, ENCRYPTED, HIDDEN, READONLY, REPARSE_POINT, SYSTEM,
};
use bb_core::columns::{ColumnCatalog, ColumnValue};
use bb_core::data::SearchHit;
use bb_core::format::byte_size;
use bb_core::models::FileEntry;
use bb_core::search::ContentMatch;
use bb_paths::PathKey;

/// The cell columns, in the order `ColumnCatalog::built_ins` lists them. A cell is asked for by its
/// index here; `ColumnId::NONE` is a column whose values this build cannot read (a shell property).
pub const CELL_COLUMNS: [&str; 11] = [
    ColumnCatalog::NAME,
    ColumnCatalog::RELATIVE_PATH,
    ColumnCatalog::MATCH,
    ColumnCatalog::COMPARE_STATUS,
    ColumnCatalog::SIZE,
    ColumnCatalog::TYPE,
    ColumnCatalog::MODIFIED,
    ColumnCatalog::CREATED,
    ColumnCatalog::ACCESSED,
    ColumnCatalog::ATTRIBUTES,
    ColumnCatalog::EXTENSION,
];

/// The index of a built-in column id in [`CELL_COLUMNS`], case-insensitively.
pub fn cell_index(id: &str) -> Option<u32> {
    CELL_COLUMNS
        .iter()
        .position(|c| c.eq_ignore_ascii_case(id))
        .map(|i| i as u32)
}

const MEDIA_EXTENSIONS: [&str; 27] = [
    ".jpg", ".jpeg", ".jfif", ".png", ".gif", ".bmp", ".webp", ".tif", ".tiff", ".ico", ".heic",
    ".heif", ".avif", ".svg", ".mp4", ".mov", ".avi", ".mkv", ".wmv", ".webm", ".m4v", ".flv",
    ".mpg", ".mpeg", ".3gp", ".m2ts", ".mts",
];

/// `Path.GetExtension`: from the last dot, or empty when there is none or it ends the name.
pub fn extension_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i + 1 < name.len() => &name[i..],
        _ => "",
    }
}

struct Wide {
    name: Box<[u16]>,
    full_path: Box<[u16]>,
    relative_dir: Box<[u16]>,
}

pub struct Row {
    /// Stable per path within a tab's browsing of one place, so hover and selection survive a
    /// merge. Zero until the tab has interned it.
    pub id: u64,
    /// `PathKey::canonicalize` of the path; what rows are matched by.
    pub key: Arc<str>,
    pub name: String,
    pub full_path: String,
    pub relative_dir: String,
    pub is_directory: bool,
    pub attributes: u32,
    /// None is a folder whose size is not known: blank, never zero.
    pub size: Option<i64>,
    pub size_incomplete: bool,
    pub size_computed: FileTime,
    pub modified: FileTime,
    pub created: FileTime,
    pub accessed: FileTime,
    pub type_name: String,
    pub is_media: bool,
    pub content_match: Option<ContentMatch>,
    /// Shell-property values the host has supplied, by upper-cased canonical name. `None` for a
    /// property the handler had no value for: asked, answered, blank.
    pub props: Option<Arc<RowProps>>,
    wide: OnceLock<Wide>,
}

/// The shell-property cells of one row.
pub type RowProps = HashMap<String, Option<ColumnValue>>;

impl Row {
    pub fn from_entry(entry: &FileEntry) -> Row {
        Row::build(entry, String::new(), None)
    }

    /// A copy with the same id and display state, for building the next snapshot of a listing.
    pub fn duplicate(&self) -> Row {
        let mut c = Row::from_entry(&self.to_entry());
        c.id = self.id;
        c.relative_dir = self.relative_dir.clone();
        c.content_match = self.content_match.clone();
        c.size = self.size;
        c.size_incomplete = self.size_incomplete;
        c.size_computed = self.size_computed;
        c.props = self.props.clone();
        c
    }

    /// The row's value for a property (upper-cased canonical name), when the host supplied one.
    pub fn prop(&self, folded: &str) -> Option<&ColumnValue> {
        self.props.as_ref()?.get(folded)?.as_ref()
    }

    pub fn from_hit(hit: &SearchHit) -> Row {
        let mut attributes = hit.attributes;
        if hit.hidden {
            attributes |= HIDDEN;
        }
        let entry = FileEntry {
            name: hit.name.clone(),
            full_path: hit.display_path.clone(),
            is_directory: hit.is_directory,
            size_bytes: if hit.is_directory { -1 } else { hit.size_bytes },
            modified_utc: hit.modified_utc,
            attributes,
            created_utc: hit.created_utc,
            accessed_utc: FileTime::UNKNOWN,
        };
        Row::build(
            &entry,
            hit.relative_dir_display.clone(),
            hit.content_match.clone(),
        )
    }

    fn build(entry: &FileEntry, relative_dir: String, content_match: Option<ContentMatch>) -> Row {
        let extension = extension_of(&entry.name);
        let type_name = if entry.is_directory {
            "Folder".to_string()
        } else if extension.len() > 1 {
            format!("{} file", extension[1..].to_uppercase())
        } else {
            "File".to_string()
        };
        let is_media = !entry.is_directory
            && MEDIA_EXTENSIONS
                .iter()
                .any(|m| m.eq_ignore_ascii_case(extension_of(&entry.full_path)));
        // A directory's size is normally unknown at listing time and arrives from dir_size_cache;
        // a lister that already knows it writes a real value, which is what >= 0 reads.
        let size = if entry.is_directory {
            (entry.size_bytes >= 0).then_some(entry.size_bytes)
        } else {
            Some(entry.size_bytes)
        };
        let key = PathKey::canonicalize(&entry.full_path)
            .unwrap_or_else(|_| entry.full_path.to_uppercase());
        Row {
            id: 0,
            key: Arc::from(key),
            name: entry.name.clone(),
            full_path: entry.full_path.clone(),
            relative_dir,
            is_directory: entry.is_directory,
            attributes: entry.attributes,
            size,
            size_incomplete: false,
            size_computed: FileTime::UNKNOWN,
            modified: entry.modified_utc,
            created: entry.created_utc,
            accessed: entry.accessed_utc,
            type_name,
            is_media,
            content_match,
            props: None,
            wide: OnceLock::new(),
        }
    }

    pub fn to_entry(&self) -> FileEntry {
        FileEntry {
            name: self.name.clone(),
            full_path: self.full_path.clone(),
            is_directory: self.is_directory,
            size_bytes: if self.is_directory {
                -1
            } else {
                self.size.unwrap_or(0)
            },
            modified_utc: self.modified,
            attributes: self.attributes,
            created_utc: self.created,
            accessed_utc: self.accessed,
        }
    }

    pub fn is_hidden(&self) -> bool {
        self.attributes & HIDDEN != 0
    }

    pub fn extension(&self) -> &str {
        if self.is_directory {
            return "";
        }
        let e = extension_of(&self.name);
        if e.len() > 1 { &e[1..] } else { "" }
    }

    /// Flags as letters, in the order the Attributes column shows them.
    pub fn attributes_text(&self) -> String {
        if self.attributes == 0 {
            return String::new();
        }
        let mut text = String::new();
        for (bit, letter) in [
            (READONLY, 'R'),
            (HIDDEN, 'H'),
            (SYSTEM, 'S'),
            (ARCHIVE, 'A'),
            (COMPRESSED, 'C'),
            (ENCRYPTED, 'E'),
            (REPARSE_POINT, 'L'),
        ] {
            if self.attributes & bit != 0 {
                text.push(letter);
            }
        }
        text
    }

    pub fn size_sort_key(&self) -> i64 {
        self.size.unwrap_or(-1)
    }

    pub fn size_text(&self) -> String {
        match self.size {
            Some(b) => format!(
                "{}{}",
                byte_size(b),
                if self.size_incomplete { " *" } else { "" }
            ),
            None if self.is_directory => "\u{2014}".to_string(),
            None => String::new(),
        }
    }

    pub fn cell_text(&self, column: u32) -> String {
        match CELL_COLUMNS.get(column as usize).copied() {
            Some(ColumnCatalog::NAME) => self.name.clone(),
            Some(ColumnCatalog::RELATIVE_PATH) => self.relative_dir.clone(),
            Some(ColumnCatalog::MATCH) => self
                .content_match
                .as_ref()
                .map(|m| m.line.clone())
                .unwrap_or_default(),
            Some(ColumnCatalog::SIZE) => self.size_text(),
            Some(ColumnCatalog::TYPE) => self.type_name.clone(),
            Some(ColumnCatalog::MODIFIED) => stamp(self.modified),
            Some(ColumnCatalog::CREATED) => stamp(self.created),
            Some(ColumnCatalog::ACCESSED) => stamp(self.accessed),
            Some(ColumnCatalog::ATTRIBUTES) => self.attributes_text(),
            Some(ColumnCatalog::EXTENSION) => self.extension().to_string(),
            _ => String::new(),
        }
    }

    pub fn name_wide(&self) -> &[u16] {
        &self.wide().name
    }

    pub fn path_wide(&self) -> &[u16] {
        &self.wide().full_path
    }

    pub fn relative_dir_wide(&self) -> &[u16] {
        &self.wide().relative_dir
    }

    fn wide(&self) -> &Wide {
        self.wide.get_or_init(|| Wide {
            name: self.name.encode_utf16().collect(),
            full_path: self.full_path.encode_utf16().collect(),
            relative_dir: self.relative_dir.encode_utf16().collect(),
        })
    }
}

/// A local date and time, blank when unknown.
pub fn stamp(utc: FileTime) -> String {
    if utc.is_unknown() {
        return String::new();
    }
    let c = utc.to_local().civil();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        c.year, c.month, c.day, c.hour, c.minute
    )
}

/// An immutable snapshot of what a tab lists, in display order.
pub struct Listing {
    pub tab: u64,
    pub version: u64,
    pub is_flattened: bool,
    pub is_flat_browse: bool,
    rows: Vec<Arc<Row>>,
    cells: Mutex<HashMap<u64, Box<[u16]>>>,
    /// The shell-property columns showing when this snapshot was made (upper-cased canonical
    /// names, in column order): cell index `CELL_COLUMNS.len() + k` is the k-th of these.
    prop_ids: Vec<String>,
}

impl Listing {
    /// The snapshot with its shell-property columns named.
    pub fn with_props(mut self, prop_ids: Vec<String>) -> Listing {
        self.prop_ids = prop_ids;
        self
    }

    pub fn prop_ids(&self) -> &[String] {
        &self.prop_ids
    }

    pub fn new(
        tab: u64,
        version: u64,
        rows: Vec<Arc<Row>>,
        is_flattened: bool,
        is_flat_browse: bool,
    ) -> Listing {
        Listing {
            tab,
            version,
            is_flattened,
            is_flat_browse,
            rows,
            cells: Mutex::new(HashMap::new()),
            prop_ids: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn rows(&self) -> &[Arc<Row>] {
        &self.rows
    }

    pub fn row(&self, index: usize) -> Option<&Arc<Row>> {
        self.rows.get(index)
    }

    pub fn index_of_id(&self, id: u64) -> Option<usize> {
        self.rows.iter().position(|r| r.id == id)
    }

    /// The cell's UTF-16 text. The view lives as long as this listing: entries are only ever added.
    pub fn cell(&self, index: usize, column: u32) -> Option<&[u16]> {
        let row = self.rows.get(index)?;
        let key = ((index as u64) << 8) | u64::from(column & 0xFF);
        let mut cells = self.cells.lock().unwrap_or_else(|p| p.into_inner());
        let wide = cells.entry(key).or_insert_with(|| {
            let text = match (column as usize).checked_sub(CELL_COLUMNS.len()) {
                None => row.cell_text(column),
                Some(k) => self
                    .prop_ids
                    .get(k)
                    .and_then(|id| row.prop(id))
                    .map(|v| v.display.clone())
                    .unwrap_or_default(),
            };
            text.encode_utf16().collect()
        });
        // SAFETY: the boxed slice's heap storage never moves or drops while the map lives, and
        // entries are never removed or replaced; the map lives exactly as long as `self`.
        let slice: &[u16] = unsafe { std::slice::from_raw_parts(wide.as_ptr(), wide.len()) };
        Some(slice)
    }
}
