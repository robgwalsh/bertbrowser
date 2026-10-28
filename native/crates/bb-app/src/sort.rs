//! The listing's order. Spec: FileListViewModel.CurrentComparer. Layout bands sort ahead of the
//! column direction: folders first in a directory listing (never in a flattened one, whose rows come
//! from many folders), and media after everything else while thumbnails show. Ties fall back to the
//! natural order of the name, and that tiebreak flips with the direction like the column does.

use std::cmp::Ordering;

use bb_core::columns::{ColumnCatalog, ColumnKind};
use bb_paths::{fold_upper, natural_cmp};

use crate::rows::Row;

#[derive(Clone, Debug, PartialEq)]
pub struct SortSpec {
    /// A catalogue id; unusable ids degrade to Name.
    pub column: String,
    pub descending: bool,
    pub flattened: bool,
    pub thumbnails: bool,
}

impl SortSpec {
    pub fn new(column: &str, descending: bool, flattened: bool, thumbnails: bool) -> SortSpec {
        SortSpec {
            column: ColumnCatalog::sort_spec(Some(column)).id,
            descending,
            flattened,
            thumbnails,
        }
    }

    fn band(&self, row: &Row) -> u8 {
        if !self.flattened && row.is_directory {
            0
        } else if self.thumbnails && row.is_media {
            2
        } else {
            1
        }
    }

    pub fn compare(&self, a: &Row, b: &Row) -> Ordering {
        let band = self.band(a).cmp(&self.band(b));
        if band != Ordering::Equal {
            return band;
        }
        let spec = ColumnCatalog::sort_spec(Some(&self.column));
        let mut ordered = if spec.kind == ColumnKind::ShellProperty {
            // Shell values are not read in this build, so every row is unknown and the name decides.
            Ordering::Equal
        } else {
            match spec.id.as_str() {
                ColumnCatalog::SIZE => a.size_sort_key().cmp(&b.size_sort_key()),
                ColumnCatalog::TYPE => fold_upper(&a.type_name).cmp(&fold_upper(&b.type_name)),
                ColumnCatalog::MODIFIED => a.modified.cmp(&b.modified),
                ColumnCatalog::CREATED => a.created.cmp(&b.created),
                ColumnCatalog::ACCESSED => a.accessed.cmp(&b.accessed),
                ColumnCatalog::ATTRIBUTES => a.attributes_text().cmp(&b.attributes_text()),
                ColumnCatalog::EXTENSION => {
                    fold_upper(a.extension()).cmp(&fold_upper(b.extension()))
                }
                ColumnCatalog::RELATIVE_PATH => natural_cmp(&a.relative_dir, &b.relative_dir),
                _ => natural_cmp(&a.name, &b.name),
            }
        };
        if ordered == Ordering::Equal {
            ordered = natural_cmp(&a.name, &b.name);
        }
        if self.descending {
            ordered.reverse()
        } else {
            ordered
        }
    }

    pub fn sort(&self, rows: &mut [Row]) {
        rows.sort_by(|a, b| self.compare(a, b));
    }
}
