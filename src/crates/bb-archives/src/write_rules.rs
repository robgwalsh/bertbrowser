//! What each write format is called, what it produces, and what this app will not produce at all.
//! Spec: Core/Services/Archives/ArchiveWriteRules.cs.
//!
//! **7z and RAR are refused by name rather than greyed out with no explanation.** Nothing here
//! writes 7z the way 7-Zip does, and nothing but WinRAR writes RAR. Both remain perfectly
//! readable; it is only creating them that is impossible.

use bb_paths::file_name;

/// The part before the last dot, as `Path.GetFileNameWithoutExtension` gave it.
fn stem(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) => &name[..i],
        None => name,
    }
}

/// A container this app can produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArchiveWriteFormat {
    Zip,
    Tar,
    TarGz,
    TarBz2,
}

/// How hard to try, in the three words a person actually means.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompressionLevel {
    /// No compression. The right answer for a folder of JPEGs or MP4s.
    Store,
    Normal,
    Maximum,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteFormatInfo {
    pub format: ArchiveWriteFormat,
    pub label: &'static str,
    pub suffix: &'static str,
    pub supports_level: bool,
}

const ALL: &[WriteFormatInfo] = &[
    WriteFormatInfo {
        format: ArchiveWriteFormat::Zip,
        label: "Zip",
        suffix: ".zip",
        supports_level: true,
    },
    WriteFormatInfo {
        format: ArchiveWriteFormat::Tar,
        label: "Tar",
        suffix: ".tar",
        supports_level: false,
    },
    WriteFormatInfo {
        format: ArchiveWriteFormat::TarGz,
        label: "Tar.gz",
        suffix: ".tar.gz",
        supports_level: false,
    },
    WriteFormatInfo {
        format: ArchiveWriteFormat::TarBz2,
        label: "Tar.bz2",
        suffix: ".tar.bz2",
        supports_level: false,
    },
];

pub struct ArchiveWriteRules;

impl ArchiveWriteRules {
    pub fn formats() -> &'static [WriteFormatInfo] {
        ALL
    }

    pub fn info(format: ArchiveWriteFormat) -> &'static WriteFormatInfo {
        ALL.iter()
            .find(|f| f.format == format)
            .expect("every write format has a row")
    }

    pub fn suffix_for(format: ArchiveWriteFormat) -> &'static str {
        Self::info(format).suffix
    }

    /// Why a format cannot be created, or None when it can. Read by the dialog so the message is
    /// the reason rather than a disabled control.
    pub fn why_not_writable(suffix: Option<&str>) -> Option<String> {
        let suffix = suffix?;
        match suffix.to_lowercase().as_str() {
            ".7z" => Some(
                "7z archives can be read but not created — no managed 7z writer exists."
                    .to_string(),
            ),
            ".rar" => Some(
                "RAR archives can be read but not created — only WinRAR can write them."
                    .to_string(),
            ),
            ".tar.xz" | ".txz" | ".tar.zst" | ".tar.lz" => {
                Some(format!("{suffix} archives can be read but not created."))
            }
            _ => None,
        }
    }

    /// The name a new archive gets from what is going into it: the folder's own name for a single
    /// folder, the file's stem for a single file, and the containing folder's name otherwise.
    pub fn suggest_name(sources: &[String], current_directory: &str) -> String {
        if let [only] = sources {
            let name = file_name(only.trim_end_matches('\\'));
            if !name.is_empty() {
                return if std::path::Path::new(only).is_dir() {
                    name.to_string()
                } else {
                    stem(name).to_string()
                };
            }
        }

        let folder = file_name(current_directory.trim_end_matches('\\'));
        if folder.is_empty() {
            "Archive".to_string()
        } else {
            folder.to_string()
        }
    }
}
