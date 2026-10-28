//! What a file holds, what an edit asks for, what the planner answers and what the executor
//! reports. Spec: Core/Services/Metadata/MetadataModels.cs.

use std::collections::BTreeMap;

use super::codec::MetadataCodec;
use super::fields::{MetadataFamily, MetadataField};
use crate::fs::EntryStamp;

/// What one file holds, in canonical values. A field with nothing in it is absent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataDocument {
    pub family: MetadataFamily,
    pub values: BTreeMap<MetadataField, String>,
    /// The file says where it was made — GPS coordinates. Not a field, because it is only ever
    /// removed, never typed.
    pub has_location: bool,
    /// The embedded cover picture — an album's sleeve — as the image file it is, or None.
    pub picture: Option<Vec<u8>>,
}

impl MetadataDocument {
    pub fn new(family: MetadataFamily, values: BTreeMap<MetadataField, String>) -> Self {
        MetadataDocument {
            family,
            values,
            has_location: false,
            picture: None,
        }
    }

    pub fn with_location(mut self, has_location: bool) -> Self {
        self.has_location = has_location;
        self
    }

    pub fn get(&self, field: MetadataField) -> &str {
        self.values.get(&field).map(String::as_str).unwrap_or("")
    }
}

/// What to change. A field that is not named is left exactly as it is — which is how a selection
/// of forty files with forty titles keeps them while their album is set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetadataEdit {
    /// Canonical values from `MetadataFields::try_normalize`; an empty one clears the field.
    pub changes: BTreeMap<MetadataField, String>,
    /// Take out everything that describes the file rather than being it, before `changes` are
    /// applied. A picture keeps its orientation and its colour profile: those are how it is drawn,
    /// and without them it turns on its side or changes colour.
    pub remove_all: bool,
    /// Take out where a picture was taken, and nothing else.
    pub remove_location: bool,
    /// Move the date taken by this many ticks — for a camera whose clock was wrong.
    pub shift_date_taken: Option<i64>,
    /// Embed this image file as the cover picture, replacing any there.
    pub set_picture: Option<Vec<u8>>,
    /// Take the cover picture out.
    pub remove_picture: bool,
}

impl MetadataEdit {
    pub fn none() -> MetadataEdit {
        MetadataEdit::default()
    }

    pub fn of(changes: &[(MetadataField, &str)]) -> MetadataEdit {
        MetadataEdit {
            changes: changes
                .iter()
                .map(|(f, v)| (*f, (*v).to_string()))
                .collect(),
            ..MetadataEdit::default()
        }
    }

    pub fn with_changes(mut self, changes: BTreeMap<MetadataField, String>) -> MetadataEdit {
        self.changes = changes;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
            && !self.remove_all
            && !self.remove_location
            && self.shift_date_taken.is_none()
            && self.set_picture.is_none()
            && !self.remove_picture
    }

    /// The part of this edit a file `codec` handles can take.
    pub fn for_codec(&self, codec: &dyn MetadataCodec) -> MetadataEdit {
        let fields = codec.fields();
        MetadataEdit {
            changes: self
                .changes
                .iter()
                .filter(|(f, _)| fields.contains(f))
                .map(|(f, v)| (*f, v.clone()))
                .collect(),
            remove_all: self.remove_all,
            remove_location: self.remove_location && codec.family() == MetadataFamily::Image,
            shift_date_taken: if fields.contains(&MetadataField::DateTaken) {
                self.shift_date_taken
            } else {
                None
            },
            set_picture: if codec.holds_picture() {
                self.set_picture.clone()
            } else {
                None
            },
            remove_picture: self.remove_picture && codec.holds_picture(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MetadataRejection {
    Missing,
    IsFolder,
    /// An entry inside an archive: there is no file on disk to rewrite.
    InsideArchive,
    /// A cloud file whose bytes are not here; reading it would download it.
    CloudPlaceholder,
    /// A link is the entry it is, not the file it points at.
    Link,
    ReadOnly,
    /// No codec writes this type.
    Unsupported,
    /// None of the fields being changed exist for this kind of file.
    NothingApplies,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedMetadataEdit {
    pub path: String,
    pub reason: MetadataRejection,
    pub message: String,
}

/// One file and the part of the edit it can take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedMetadataEdit {
    pub path: String,
    pub edit: MetadataEdit,
    pub length: i64,
    /// Written by Windows' property handler rather than a codec — the one path with no payload
    /// check. See `PropertyFallback`.
    pub via_windows: bool,
}

/// What editing a selection would amount to. `rewrite_bytes` is every planned file whole: a tag
/// cannot be changed without writing the file out again beside itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetadataEditPlan {
    pub files: Vec<PlannedMetadataEdit>,
    pub rejected: Vec<RejectedMetadataEdit>,
}

impl MetadataEditPlan {
    pub fn empty() -> MetadataEditPlan {
        MetadataEditPlan::default()
    }

    pub fn has_work(&self) -> bool {
        !self.files.is_empty()
    }

    pub fn rewrite_bytes(&self) -> i64 {
        self.files.iter().map(|f| f.length).sum()
    }
}

/// A file with its other version held beside it: the original while an edit is in effect, the
/// edited one while it is undone. `in_place` is what is at `path` now; a step refuses a file that
/// no longer matches, since swapping it out would undo somebody else's change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldVersion {
    pub path: String,
    pub held_path: String,
    pub in_place: EntryStamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedMetadataEdit {
    pub path: String,
    pub message: String,
    pub access_denied: bool,
}

impl FailedMetadataEdit {
    pub fn new(path: &str, message: String) -> FailedMetadataEdit {
        FailedMetadataEdit {
            path: path.to_string(),
            message,
            access_denied: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetadataEditOutcome {
    pub edited: Vec<HeldVersion>,
    pub failed: Vec<FailedMetadataEdit>,
    pub cancelled: bool,
    /// Files the edit was offered to and which already were as it asked: not changed, not
    /// failed, and not in the history.
    pub unchanged: Vec<String>,
}

impl MetadataEditOutcome {
    pub fn empty() -> MetadataEditOutcome {
        MetadataEditOutcome::default()
    }

    pub fn can_undo(&self) -> bool {
        !self.edited.is_empty()
    }
}

/// What an undo or a redo moved, and what it could not.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetadataSwapResult {
    pub swapped: Vec<HeldVersion>,
    pub failed: Vec<FailedMetadataEdit>,
}
