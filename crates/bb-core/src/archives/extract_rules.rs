//! The pure decisions an extract needs: where "here" is, and what one entry's destination path is.
//! Spec: Core/Services/Archives/ExtractRules.cs.

use bb_paths::{UniquePath, file_name, split_last};

use super::{ArchiveFormats, ArchiveIndex, ArchivePath, ExtractRejection};

pub struct ExtractRules;

impl ExtractRules {
    /// Where "Extract here" puts things. **Tarbomb avoidance, the right way round.** An archive
    /// whose contents sit under a single top-level folder already carries its own wrapper, and
    /// putting that inside a second folder named after the file gives `project\project\src`. One
    /// that does not, the tarbomb, would otherwise spray its files across the folder being
    /// browsed, so that one gets a wrapper named after the archive, deduplicated by `UniquePath`.
    pub fn destination_for(
        index: &ArchiveIndex,
        archive_file: &str,
        directory_exists: impl Fn(&str) -> bool,
        file_exists: impl Fn(&str) -> bool,
    ) -> String {
        let beside = match split_last(archive_file) {
            ("", _) => archive_file,
            (parent, _) => parent,
        };
        let top = index.children("").unwrap_or_default();

        if let [only] = top.as_slice()
            && only.is_directory
        {
            return beside.to_string();
        }

        UniquePath::for_path(
            &format!("{beside}\\{}", Self::stem_of(archive_file)),
            true,
            directory_exists,
            file_exists,
        )
    }

    /// The archive's name without its suffix: `backup.tar.gz` gives `backup`, not `backup.tar`,
    /// because the compound suffix is one thing and half of it is not a name.
    pub fn stem_of(archive_file: &str) -> String {
        let name = file_name(archive_file);
        if let Some(format) = ArchiveFormats::match_name(name) {
            return name[..name.len() - format.suffix.len()].to_string();
        }
        match name.rfind('.') {
            Some(dot) => name[..dot].to_string(),
            None => name.to_string(),
        }
    }

    /// Where one entry lands, given the subtree being extracted from. `relative_to` is the folder
    /// inside the archive the extract is rooted at, so extracting `src` from inside it produces
    /// `dest\lib\util.js` rather than `dest\src\lib\util.js`. Empty for the archive's own root.
    pub fn target_for(entry_path: &str, relative_to: &str, destination_directory: &str) -> String {
        let relative = Self::relative(entry_path, relative_to);
        format!(
            "{}\\{}",
            destination_directory.trim_end_matches('\\'),
            relative
        )
    }

    pub(crate) fn relative<'a>(entry_path: &'a str, relative_to: &str) -> &'a str {
        if relative_to.is_empty() {
            return entry_path;
        }
        let prefix = format!("{}\\", relative_to.trim_end_matches('\\'));
        if entry_path.len() >= prefix.len()
            && entry_path.is_char_boundary(prefix.len())
            && entry_path[..prefix.len()].eq_ignore_ascii_case(&prefix)
        {
            &entry_path[prefix.len()..]
        } else {
            file_name(entry_path)
        }
    }

    /// Whether a destination is safe to extract into: a real folder, and not inside the container
    /// being read from. A virtual path canonicalizes happily, so "extract this zip into itself"
    /// produces a destination no executor could ever write to; better refused by name than failed
    /// one file at a time.
    pub fn reject_destination(
        destination_directory: &str,
        directory_exists: impl Fn(&str) -> bool,
        file_exists: impl Fn(&str) -> bool,
    ) -> Option<ExtractRejection> {
        if destination_directory.trim().is_empty() {
            return Some(ExtractRejection::DestinationMissing);
        }
        if ArchivePath::looks_virtual(destination_directory)
            && !directory_exists(destination_directory)
        {
            return Some(ExtractRejection::DestinationInsideArchive);
        }
        if file_exists(destination_directory) {
            return Some(ExtractRejection::DestinationNotDirectory);
        }
        None
    }
}
