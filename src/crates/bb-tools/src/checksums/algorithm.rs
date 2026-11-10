//! The hash algorithms the checksum tool offers, and the one table that says what each is called,
//! how long its digest is and which checksum-file extension belongs to it — so a label, a file
//! name and a parser cannot disagree. Spec: Core/Services/Checksums/ChecksumAlgorithm.cs.

use serde::{Deserialize, Serialize};

/// The union of what Directory Opus, Total Commander, XYplorer, Double Commander and Multi
/// Commander publish, which is what makes a checksum from any of them verifiable here. Ordered
/// deliberately: CRC-32 catches corruption and nothing else, and sits at the end of a list rather
/// than the front of one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChecksumAlgorithm {
    Md5,
    Sha1,
    Sha256,
    Sha512,
    Crc32,
}

impl ChecksumAlgorithm {
    /// Every algorithm, in the order a UI should offer them.
    pub const ALL: [ChecksumAlgorithm; 5] = ChecksumAlgorithms::ALL;

    /// The enum's own name, as the settings file writes it.
    pub fn name(self) -> &'static str {
        match self {
            ChecksumAlgorithm::Md5 => "Md5",
            ChecksumAlgorithm::Sha1 => "Sha1",
            ChecksumAlgorithm::Sha256 => "Sha256",
            ChecksumAlgorithm::Sha512 => "Sha512",
            ChecksumAlgorithm::Crc32 => "Crc32",
        }
    }

    /// `Enum.TryParse(name, ignoreCase: true)`.
    pub fn parse_ignore_case(name: &str) -> Option<ChecksumAlgorithm> {
        Self::ALL
            .into_iter()
            .find(|a| a.name().eq_ignore_ascii_case(name.trim()))
    }

    /// The requested set in the offered order, once each. See [`ChecksumAlgorithms::normalise`].
    pub fn normalise(
        requested: impl IntoIterator<Item = ChecksumAlgorithm>,
    ) -> Vec<ChecksumAlgorithm> {
        ChecksumAlgorithms::normalise(requested)
    }
}

/// The table.
pub struct ChecksumAlgorithms;

impl ChecksumAlgorithms {
    /// Every algorithm, in the order a UI should offer them.
    pub const ALL: [ChecksumAlgorithm; 5] = [
        ChecksumAlgorithm::Md5,
        ChecksumAlgorithm::Sha1,
        ChecksumAlgorithm::Sha256,
        ChecksumAlgorithm::Sha512,
        ChecksumAlgorithm::Crc32,
    ];

    /// What a person calls it.
    pub fn display_name(algorithm: ChecksumAlgorithm) -> &'static str {
        match algorithm {
            ChecksumAlgorithm::Crc32 => "CRC-32",
            ChecksumAlgorithm::Md5 => "MD5",
            ChecksumAlgorithm::Sha1 => "SHA-1",
            ChecksumAlgorithm::Sha256 => "SHA-256",
            ChecksumAlgorithm::Sha512 => "SHA-512",
        }
    }

    /// Characters in the hex digest. Distinct across all five, which is the whole reason
    /// [`from_digest_length`](Self::from_digest_length) can work.
    pub fn hex_length(algorithm: ChecksumAlgorithm) -> usize {
        match algorithm {
            ChecksumAlgorithm::Crc32 => 8,
            ChecksumAlgorithm::Md5 => 32,
            ChecksumAlgorithm::Sha1 => 40,
            ChecksumAlgorithm::Sha256 => 64,
            ChecksumAlgorithm::Sha512 => 128,
        }
    }

    /// The conventional extension of a checksum file holding this algorithm.
    pub fn file_extension(algorithm: ChecksumAlgorithm) -> &'static str {
        match algorithm {
            ChecksumAlgorithm::Crc32 => ".sfv",
            ChecksumAlgorithm::Md5 => ".md5",
            ChecksumAlgorithm::Sha1 => ".sha1",
            ChecksumAlgorithm::Sha256 => ".sha256",
            ChecksumAlgorithm::Sha512 => ".sha512",
        }
    }

    /// Whether a checksum file of this algorithm is conventionally written in lower case.
    ///
    /// The *only* place casing is decided. Digests are uppercase everywhere inside this app — the
    /// sinks produce them that way and the duplicate finder groups on them — and are lowered solely
    /// on the way out to a file, because that is what `sha256sum` writes and what a person diffing
    /// two checksum files expects. SFV is the exception: its convention is upper case. Reading never
    /// consults this, since every comparison in the app ignores case.
    pub fn is_written_lowercase(algorithm: ChecksumAlgorithm) -> bool {
        algorithm != ChecksumAlgorithm::Crc32
    }

    /// The algorithm a checksum file's name implies, or None when the name says nothing. Takes a
    /// file name or a bare extension; either works.
    pub fn from_extension(file_name_or_extension: Option<&str>) -> Option<ChecksumAlgorithm> {
        let text = file_name_or_extension.filter(|t| !t.is_empty())?;

        let extension = if text.starts_with('.') && !text.contains('\\') {
            text
        } else {
            get_extension(text)
        };

        Self::ALL
            .into_iter()
            .find(|&a| extension.eq_ignore_ascii_case(Self::file_extension(a)))
    }

    /// The algorithm a digest of this many hex characters must have come from, or None.
    ///
    /// This is what lets the compare box work out for itself what someone pasted: the five lengths
    /// do not collide, so a 32-character paste is an MD5 and nothing else. Length alone is not
    /// proof the text *is* a digest — the caller checks it is hex — only of which algorithm it
    /// would belong to.
    pub fn from_digest_length(hex_length: usize) -> Option<ChecksumAlgorithm> {
        Self::ALL
            .into_iter()
            .find(|&a| Self::hex_length(a) == hex_length)
    }

    /// Whether every character is a hex digit, and there is at least one.
    pub fn is_hex(text: &str) -> bool {
        !text.is_empty() && text.chars().all(|c| c.is_ascii_hexdigit())
    }

    /// The algorithm a token looks like it belongs to: hex, of a length only one algorithm uses.
    pub fn recognise(token: &str) -> Option<ChecksumAlgorithm> {
        if Self::is_hex(token) {
            Self::from_digest_length(token.len())
        } else {
            None
        }
    }

    /// A requested set reduced to each algorithm once, in [`ALL`](Self::ALL) order.
    ///
    /// Callers pass a list because a set has no order and the results are rendered in one. Doing
    /// the reduction here rather than at each call site is what stops two windows disagreeing
    /// about the order they show digests in.
    pub fn normalise(
        requested: impl IntoIterator<Item = ChecksumAlgorithm>,
    ) -> Vec<ChecksumAlgorithm> {
        let wanted: Vec<ChecksumAlgorithm> = requested.into_iter().collect();
        Self::ALL
            .into_iter()
            .filter(|a| wanted.contains(a))
            .collect()
    }
}

/// `Path.GetExtension`: the last dot of the last component, with the dot, or empty when the
/// component has none or ends in one.
fn get_extension(path: &str) -> &str {
    let name = bb_paths::file_name(path);
    match name.rfind('.') {
        Some(i) if i + 1 < name.len() => &name[i..],
        _ => "",
    }
}
