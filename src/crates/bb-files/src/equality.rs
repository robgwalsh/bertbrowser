//! Deciding whether two entries are the same file. Pure, and the only place the definition lives:
//! here, below both the transfer ("identical") and the folder comparison ("same"), which both ask it.
//! Spec: Core/Services/Compare/CompareEquality.cs, CompareModels.cs (the entry and the verdict).

use bb_base::FileTime;
use bb_base::time::{TICKS_PER_HOUR, TICKS_PER_SECOND};

/// One entry on one side of a comparison, addressed relative to that side's root.
///
/// `relative_key` is uppercased, `\`-separated, with no leading separator: a path key with the
/// root sliced off. Pairing the two sides by this string is what makes the comparison agree with
/// the rest of the app about what "the same path" means. There is deliberately no whole display
/// path per entry: over a real 1.65-million-row index that second copy of every path is the
/// largest avoidable thing a comparison holds, so `CompareResult::display_path` rebuilds the few that reach a dialog from the ancestors'
/// names. `modified_utc` of [`FileTime::UNKNOWN`] means *unknown* (the index's name-only build path
/// writes exactly that) and forces [`CompareVerdict::Unknown`] rather than a guess.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompareEntry {
    pub relative_key: String,
    /// The leaf name, display-cased.
    pub name: String,
    pub is_directory: bool,
    /// Bytes for a file; ignored for a folder, whose size is never compared.
    pub size_bytes: i64,
    pub modified_utc: FileTime,
}

impl CompareEntry {
    pub fn new(
        relative_key: impl Into<String>,
        name: impl Into<String>,
        is_directory: bool,
        size_bytes: i64,
        modified_utc: FileTime,
    ) -> CompareEntry {
        CompareEntry {
            relative_key: relative_key.into(),
            name: name.into(),
            is_directory,
            size_bytes,
            modified_utc,
        }
    }
}

/// How the two sides of one relative path relate. Side-neutral: it names the relationship, not
/// what either list should look like; `CompareRules::row_state`
/// projects it per side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompareVerdict {
    /// Could not be established, because a side reported no timestamp. Never rendered as a match
    /// and never turned into a sync action: this is the one verdict that must never be produced by
    /// guessing, because `Same` is what authorises a delete.
    Unknown,
    /// Both sides exist and match.
    Same,
    /// Present on the left, absent on the right.
    LeftOnly,
    /// Present on the right, absent on the left.
    RightOnly,
    /// Both exist; the left is measurably newer.
    LeftNewer,
    /// Both exist; the right is measurably newer.
    RightNewer,
    /// Both exist and are not the same, but neither is measurably newer: equal timestamps with
    /// unequal sizes, or a file on one side and a folder on the other.
    Differs,
}

impl CompareVerdict {
    /// Every verdict, for the tests that sweep them as `Enum.GetValues` did.
    pub const ALL: [CompareVerdict; 7] = [
        CompareVerdict::Unknown,
        CompareVerdict::Same,
        CompareVerdict::LeftOnly,
        CompareVerdict::RightOnly,
        CompareVerdict::LeftNewer,
        CompareVerdict::RightNewer,
        CompareVerdict::Differs,
    ];
}

/// How far apart two timestamps may be and still count as the same instant.
///
/// `slack` is two seconds, because FAT and exFAT store a write time to two-second granularity: a
/// file copied from NTFS onto a USB stick comes back with a timestamp up to two seconds off and
/// would otherwise read as "older" for ever, on every pass, for every file. `allow_whole_hour_shift`
/// also forgives a gap of a whole hour or two, to within `slack`: FAT stores local time with no
/// zone, so every file on the stick appears to shift by an hour when the clocks change.
///
/// The hour rule is deliberately a **whole-hour** rule and not a one-hour band. A blanket ±1 h
/// tolerance would call a file that really is fifty minutes newer "the same", which is a wrong
/// answer in the dangerous direction: "same" is what authorises deleting the other side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompareTolerance {
    /// In FILETIME ticks.
    pub slack: i64,
    pub allow_whole_hour_shift: bool,
}

impl CompareTolerance {
    /// Filesystem granularity only. The default, and what an unrecognised volume gets.
    pub const STRICT: CompareTolerance = CompareTolerance {
        slack: CompareEquality::FAT_GRANULARITY,
        allow_whole_hour_shift: false,
    };

    /// Granularity plus the whole-hour shift, for a FAT-family volume.
    pub const LOOSE: CompareTolerance = CompareTolerance {
        slack: CompareEquality::FAT_GRANULARITY,
        allow_whole_hour_shift: true,
    };

    /// The tolerance for a pair of volumes, from each side's filesystem name (`NTFS`, `FAT32`,
    /// `exFAT`...). Takes the strings rather than reading the volumes itself, so the rule stays
    /// pure and testable. **Fails closed to [`STRICT`](Self::STRICT)** when a format is `None` or
    /// unrecognised: a UNC path, a mapped drive, a volume that would not answer. An unforgiven hour
    /// shows as "newer" and offers a copy, which is additive and harmless; a wrongly forgiven one
    /// shows as "same" and offers a delete.
    pub fn for_formats(left_format: Option<&str>, right_format: Option<&str>) -> CompareTolerance {
        if Self::is_fat_family(left_format) || Self::is_fat_family(right_format) {
            Self::LOOSE
        } else {
            Self::STRICT
        }
    }

    fn is_fat_family(format: Option<&str>) -> bool {
        format.is_some_and(|f| {
            (f.len() >= 3 && f.is_char_boundary(3) && f[..3].eq_ignore_ascii_case("FAT"))
                || f.eq_ignore_ascii_case("exFAT")
        })
    }
}

pub struct CompareEquality;

impl CompareEquality {
    /// FAT and exFAT round a write time to this.
    pub const FAT_GRANULARITY: i64 = 2 * TICKS_PER_SECOND;

    const FORGIVEN_SHIFTS: [i64; 2] = [TICKS_PER_HOUR, 2 * TICKS_PER_HOUR];

    /// -1, 0 or +1, with zero meaning "the same instant as far as this tolerance can tell", so
    /// "newer" always means *measurably* newer.
    pub fn compare_times(a: FileTime, b: FileTime, tolerance: CompareTolerance) -> i32 {
        let gap = a.ticks_since(b);
        let magnitude = gap.abs();

        if magnitude <= tolerance.slack {
            return 0;
        }

        if tolerance.allow_whole_hour_shift
            && Self::FORGIVEN_SHIFTS
                .iter()
                .any(|shift| (magnitude - shift).abs() <= tolerance.slack)
        {
            return 0;
        }

        if gap > 0 { 1 } else { -1 }
    }

    /// The verdict for one relative path. A `None` side means the entry is absent there.
    ///
    /// Two directories always come back [`CompareVerdict::Same`]: a folder has no size worth
    /// comparing and its own timestamp moves for reasons nobody cares about, so its real verdict
    /// is the roll-up of everything beneath it, which `FolderComparer::compare` applies afterwards.
    /// `Same` is the correct seed for that fold, and is also the right answer for two empty folders.
    pub fn verdict(
        left: Option<&CompareEntry>,
        right: Option<&CompareEntry>,
        tolerance: CompareTolerance,
    ) -> CompareVerdict {
        let Some(l) = left else {
            return if right.is_none() {
                CompareVerdict::Unknown
            } else {
                CompareVerdict::RightOnly
            };
        };
        let Some(r) = right else {
            return CompareVerdict::LeftOnly;
        };

        if l.is_directory != r.is_directory {
            return CompareVerdict::Differs;
        }
        if l.is_directory {
            return CompareVerdict::Same;
        }

        // The index's name-only build path writes the unknown timestamp for every entry on the
        // volume. Comparing sizes alone there would call a whole drive "the same" and authorise
        // deleting it.
        if l.modified_utc.is_unknown() || r.modified_utc.is_unknown() {
            return CompareVerdict::Unknown;
        }

        let by_time = Self::compare_times(l.modified_utc, r.modified_utc, tolerance);
        if by_time > 0 {
            return CompareVerdict::LeftNewer;
        }
        if by_time < 0 {
            return CompareVerdict::RightNewer;
        }

        // Same instant. Only the size can still separate them, and when it does neither side can
        // be called newer, which is exactly what Differs is for.
        if l.size_bytes == r.size_bytes {
            CompareVerdict::Same
        } else {
            CompareVerdict::Differs
        }
    }
}
