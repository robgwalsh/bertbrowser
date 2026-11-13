//! The listing's order. Spec: FileListViewModel.CurrentComparer. Layout bands sort ahead of the
//! column direction: folders first in a directory listing (never in a flattened one, whose rows come
//! from many folders), and media after everything else while thumbnails show. Ties fall back to the
//! natural order of the name, and that tiebreak flips with the direction like the column does.

use std::cmp::Ordering;

use bb_core::columns::{ColumnCatalog, ColumnComparison, ColumnKind};
use bb_paths::{fold_upper, fold_upper_cmp, natural_cmp};

use crate::browse::rows::Row;

#[derive(Clone, Debug, PartialEq)]
pub struct SortSpec {
    /// A catalogue id; unusable ids degrade to Name.
    pub column: String,
    pub descending: bool,
    pub flattened: bool,
    pub thumbnails: bool,
    /// The column resolved once, here: a sort compares n log n times, and asking the catalogue on
    /// each comparison was several allocations apiece.
    key: SortKey,
}

#[derive(Clone, Debug, PartialEq)]
enum SortKey {
    Name,
    Size,
    Type,
    Modified,
    Created,
    Accessed,
    Attributes,
    Extension,
    RelativePath,
    /// A shell property, by its upper-cased canonical name.
    Property(String),
}

impl SortSpec {
    pub fn new(column: &str, descending: bool, flattened: bool, thumbnails: bool) -> SortSpec {
        let spec = ColumnCatalog::sort_spec(Some(column));
        let key = if spec.kind == ColumnKind::ShellProperty {
            SortKey::Property(fold_upper(&spec.id))
        } else {
            match spec.id.as_str() {
                ColumnCatalog::SIZE => SortKey::Size,
                ColumnCatalog::TYPE => SortKey::Type,
                ColumnCatalog::MODIFIED => SortKey::Modified,
                ColumnCatalog::CREATED => SortKey::Created,
                ColumnCatalog::ACCESSED => SortKey::Accessed,
                ColumnCatalog::ATTRIBUTES => SortKey::Attributes,
                ColumnCatalog::EXTENSION => SortKey::Extension,
                ColumnCatalog::RELATIVE_PATH => SortKey::RelativePath,
                _ => SortKey::Name,
            }
        };
        SortSpec {
            column: spec.id,
            descending,
            flattened,
            thumbnails,
            key,
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
        let mut ordered = match &self.key {
            SortKey::Property(key) => {
                let (left, right) = (a.prop(key), b.prop(key));
                // A missing value sorts last in both directions: a blank is not a small value.
                let known =
                    ColumnComparison::known_band(left).cmp(&ColumnComparison::known_band(right));
                if known != Ordering::Equal {
                    return known;
                }
                ColumnComparison::compare(left, right, natural_cmp)
            }
            SortKey::Size => a.size_sort_key().cmp(&b.size_sort_key()),
            SortKey::Type => a.type_cmp(b),
            SortKey::Modified => a.modified.cmp(&b.modified),
            SortKey::Created => a.created.cmp(&b.created),
            SortKey::Accessed => a.accessed.cmp(&b.accessed),
            SortKey::Attributes => a.attributes_text().cmp(&b.attributes_text()),
            SortKey::Extension => fold_upper_cmp(a.extension(), b.extension()),
            SortKey::RelativePath => natural_cmp(a.relative_dir(), b.relative_dir()),
            SortKey::Name => natural_cmp(&a.name, &b.name),
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
