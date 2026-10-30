//! Deciding whether two files are worth diffing as text. Spec:
//! Core/Services/Compare/FileComparePlan.cs.
//!
//! A rule rather than an `if` in the window, so it can be tested, and so the answer to "why is
//! this showing hex?" is a function with a name rather than a condition buried in a view.

use bb_paths::file_name;

use crate::preview::{PreviewClassifier, PreviewKind};

/// How a pair of files should be shown side by side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileCompareMode {
    /// A line diff.
    Text,
    /// A hex dump around the first differing byte.
    Binary,
}

/// What one side of a pair turned out to be, as far as anything can tell before reading.
/// `looks_like_text` is whether decoding the front of it produced convincing text, from the same
/// sniff the preview pane trusts, so the pane and the comparison cannot come to different
/// conclusions about what a file is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileCompareSide {
    /// Its file name, for the extension table.
    pub name: String,
    pub looks_like_text: bool,
}

impl FileCompareSide {
    pub fn new(name: impl Into<String>, looks_like_text: bool) -> FileCompareSide {
        FileCompareSide {
            name: name.into(),
            looks_like_text,
        }
    }
}

pub struct FileComparePlan;

impl FileComparePlan {
    /// [`FileCompareMode::Text`] only when *both* sides can be read as text.
    ///
    /// Both, not either. A `.txt` against a `.exe` diffed as text would render one side as a
    /// screen of mojibake and invite the reader to conclude something about it; the honest answer
    /// for a mixed pair is the one that works for the harder side. A side counts as text if its
    /// extension says so (the same table the preview pane trusts, so an empty `.cs` or one full of
    /// unusual characters is still code) or if, failing that, what was decoded is convincing on
    /// its own. That second rung is what makes an extensionless `README` or a `Dockerfile` work.
    pub fn mode_for(
        left: &FileCompareSide,
        right: &FileCompareSide,
        force_text: bool,
    ) -> FileCompareMode {
        if force_text {
            return FileCompareMode::Text;
        }

        if Self::is_text(left) && Self::is_text(right) {
            FileCompareMode::Text
        } else {
            FileCompareMode::Binary
        }
    }

    /// The preview classifier answers Text for a name with no extension at all, on the grounds
    /// that such a file is usually a Dockerfile or a LICENSE and reading one as text is harmless.
    /// That reasoning does not carry here: a wrong guess in a pane is one panel of mojibake the
    /// reader can dismiss, while a wrong guess here produces two of them side by side with
    /// differences marked between them, which reads as a finding. So the extensionless case falls
    /// through to what was actually decoded, and every other answer the classifier gives is used
    /// as it stands, so there is still one extension table.
    fn is_text(side: &FileCompareSide) -> bool {
        let named = !extension(&side.name).is_empty()
            && PreviewClassifier::kind_for(&side.name) == PreviewKind::Text;

        named || side.looks_like_text
    }
}

/// `Path.GetExtension`: the last dot of the leaf name onwards, or empty; a leading-dot name is
/// all extension.
fn extension(name: &str) -> &str {
    let leaf = file_name(name);
    match leaf.rfind('.') {
        Some(i) => &leaf[i..],
        None => "",
    }
}
