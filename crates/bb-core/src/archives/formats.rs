//! The one table of archive suffixes. The preview classifier, the path parser, the navigation gate,
//! the context menu and the harness all read it, so none of them can disagree about what an archive
//! is. Matching is by **longest suffix**: `backup.tar.gz` must match `.tar.gz`, not `.gz`.
//! Spec: Core/Services/Archives/ArchiveFormats.cs.

/// The container a suffix names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArchiveContainer {
    Zip,
    SevenZip,
    Rar,
    Tar,
    GZip,
}

/// What kind of container a suffix names, and what can be done with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveFormat {
    /// Lowercase, leading dot, possibly compound (`.tar.gz`).
    pub suffix: &'static str,
    pub container: ArchiveContainer,
    /// Whether entries can be addressed individually rather than by reading from the start. A hint
    /// for menu enablement, never the authority on cost: a solid 7z is sequential in practice.
    pub random_access: bool,
    /// Whether this app can produce or rewrite one.
    pub writable: bool,
    /// Whether the preview pane may list it on selection. False for the sequential formats: listing a
    /// `.tar.gz` means decompressing the whole stream, and the pane runs on arrow keys.
    pub preview_by_listing: bool,
}

const fn f(
    suffix: &'static str,
    container: ArchiveContainer,
    random_access: bool,
    writable: bool,
    preview_by_listing: bool,
) -> ArchiveFormat {
    ArchiveFormat {
        suffix,
        container,
        random_access,
        writable,
        preview_by_listing,
    }
}

use ArchiveContainer::*;

const ALL: &[ArchiveFormat] = &[
    // Compound tar containers. Sequential by construction: the whole stream decompresses.
    f(".tar.gz", Tar, false, true, false),
    f(".tar.bz2", Tar, false, true, false),
    f(".tar.xz", Tar, false, false, false),
    f(".tar.lz", Tar, false, false, false),
    f(".tar.zst", Tar, false, false, false),
    f(".tgz", Tar, false, true, false),
    f(".tbz2", Tar, false, true, false),
    f(".txz", Tar, false, false, false),
    // The zip family: a central directory, so entries are addressable.
    f(".zip", Zip, true, true, true),
    f(".zipx", Zip, true, true, true),
    f(".nupkg", Zip, true, true, true),
    f(".snupkg", Zip, true, true, true),
    f(".jar", Zip, true, true, true),
    f(".war", Zip, true, true, true),
    f(".aar", Zip, true, true, true),
    f(".vsix", Zip, true, true, true),
    f(".apk", Zip, true, true, true),
    f(".whl", Zip, true, true, true),
    f(".crx", Zip, true, false, true),
    f(".epub", Zip, true, false, true),
    f(".xpi", Zip, true, true, true),
    f(".oxt", Zip, true, true, true),
    // Read-only formats carrying an index of their own.
    f(".7z", SevenZip, true, false, true),
    f(".rar", Rar, true, false, true),
    // Plain tar: addressable, though only by walking every header.
    f(".tar", Tar, true, true, true),
    // One compressed file, which carries the inner name.
    f(".gz", GZip, false, true, false),
];

pub struct ArchiveFormats;

impl ArchiveFormats {
    /// The longest suffix of `name` that names an archive, or None.
    pub fn match_name(name: &str) -> Option<&'static ArchiveFormat> {
        if name.is_empty() {
            return None;
        }
        let mut best: Option<&'static ArchiveFormat> = None;
        for format in ALL {
            if !ends_with_ignore_ascii_case(name, format.suffix) {
                continue;
            }
            // A name that is only the suffix (".zip") is a dotfile, not an archive.
            if name.len() == format.suffix.len() {
                continue;
            }
            if best.is_none_or(|b| format.suffix.len() > b.suffix.len()) {
                best = Some(format);
            }
        }
        best
    }

    /// Whether the name looks like something this app can browse into.
    pub fn is_archive_name(name: &str) -> bool {
        Self::match_name(name).is_some()
    }

    /// Every suffix, for tests and the syntax card.
    pub fn known() -> &'static [ArchiveFormat] {
        ALL
    }
}

fn ends_with_ignore_ascii_case(s: &str, suffix: &str) -> bool {
    s.len() >= suffix.len()
        && s.is_char_boundary(s.len() - suffix.len())
        && s[s.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}
