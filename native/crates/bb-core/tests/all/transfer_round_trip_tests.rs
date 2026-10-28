// Port of tests/BertBrowser.Core.Tests/TransferRoundTripTests.cs. Property tests over randomly
// generated trees and randomly chosen drops. Rather than checking specific behaviours, these
// assert the two invariants the feature exists to hold: a move never changes which file contents
// exist anywhere under the working root (files change location; nothing is created, truncated or
// destroyed), and undoing a move restores the tree byte-for-byte, including anything a Replace
// displaced. Several seeds are run so each shape (nested selections, name collisions, folders
// dropped near their own subtree) comes up across the matrix. The generator is a small
// deterministic PRNG rather than .NET's `Random`, so a seed names a different tree than it did in
// C#; the invariants are what is pinned, not the trees.
use std::collections::BTreeMap;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use bb_core::cancel::CancelToken;
use bb_core::fs::DirectoryRemoval;
use bb_core::transfer::{
    ConflictResolution, Resolutions, TransferExecutor, TransferOutcome, TransferPlanner,
    TransferVerb,
};
use bb_paths::PathKey;
use bb_test_support::unique_temp_path;

/// SplitMix64: deterministic per seed, with none of .NET's sequence to reproduce.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1))
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// `Random.Next(max)`: 0 up to but excluding `max`.
    fn next(&mut self, max: usize) -> usize {
        (self.next_u64() % max as u64) as usize
    }

    /// `Random.Next(min, max)`.
    fn range(&mut self, min: usize, max: usize) -> usize {
        min + self.next(max - min)
    }
}

fn resolution(rng: &mut Rng) -> ConflictResolution {
    match rng.next(3) {
        0 => ConflictResolution::Skip,
        1 => ConflictResolution::KeepBoth,
        _ => ConflictResolution::Replace,
    }
}

struct Fixture {
    root: PathBuf,
    planner: TransferPlanner,
    executor: TransferExecutor,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-roundtrip", "");
        std::fs::create_dir_all(&root).unwrap();
        Fixture {
            root,
            planner: TransferPlanner::default(),
            executor: TransferExecutor::default(),
        }
    }

    fn s(&self, path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    // --- random world building ---

    fn build_tree(&self, rng: &mut Rng) {
        // A handful of folders, deliberately including repeated names so drops collide, and
        // nested ones so a selection can contain both a folder and something inside it.
        let folder_names = ["alpha", "beta", "gamma", "shared", "alpha"];
        let mut directories = vec![self.root.clone()];

        for i in 0..10 {
            let parent = directories[rng.next(directories.len())].clone();
            let mut path = parent.join(folder_names[rng.next(folder_names.len())]);
            if path.is_dir() {
                path = PathBuf::from(format!("{}-{i}", self.s(&path)));
            }
            std::fs::create_dir_all(&path).unwrap();
            directories.push(path);
        }

        let file_names = [
            "notes.txt",
            "data.bin",
            "readme.md",
            "notes.txt",
            "image.png",
        ];
        for i in 0..30 {
            let parent = directories[rng.next(directories.len())].clone();
            let name = file_names[rng.next(file_names.len())];
            let mut path = parent.join(name);
            if path.is_file() {
                path = parent.join(format!("{i}-{name}"));
            }
            std::fs::write(&path, format!("content-{i}-{}", rng.next_u64())).unwrap();
        }
    }

    /// Picks a random handful of entries and a random destination folder, plans it, and executes
    /// whatever the planner allowed. None when the planner refused everything.
    fn perform_random_drop(
        &self,
        rng: &mut Rng,
        verb: TransferVerb,
        resolution: ConflictResolution,
    ) -> Option<TransferOutcome> {
        let entries: Vec<PathBuf> = self
            .all_entries()
            .into_iter()
            .filter(|p| !is_in_staging(&self.s(p)))
            .collect();
        let mut folders: Vec<PathBuf> = entries.iter().filter(|p| p.is_dir()).cloned().collect();
        folders.push(self.root.clone());
        if entries.is_empty() || folders.is_empty() {
            return None;
        }

        let mut sources: Vec<String> = Vec::new();
        for _ in 0..rng.range(1, 5) {
            let pick = self.s(&entries[rng.next(entries.len())]);
            if !sources.contains(&pick) {
                sources.push(pick);
            }
        }
        let destination = self.s(&folders[rng.next(folders.len())]);

        let plan = self.planner.plan(&sources, &destination, verb);
        if !plan.has_work() {
            return None;
        }

        let resolutions: Resolutions = plan
            .transfers
            .iter()
            .map(|t| (PathKey::canonicalize(&t.source_path).unwrap(), resolution))
            .collect();
        let outcome =
            self.executor
                .execute(&plan, Some(&resolutions), &CancelToken::none(), None, None);

        // A refusal is fine; a crash-shaped failure is not.
        for f in &outcome.failed {
            assert!(!f.message.trim().is_empty());
        }
        Some(outcome)
    }

    // --- observation ---

    /// Every file and folder under the root, however deep, in no particular order.
    fn all_entries(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut pending = vec![self.root.clone()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let is_dir = entry.metadata().unwrap().file_attributes() & 0x10 != 0;
                out.push(path.clone());
                if is_dir {
                    pending.push(path);
                }
            }
        }
        out
    }

    /// How many files hold each distinct content, anywhere under the root. Location-free, so it is
    /// unchanged by a move and only grows on a copy.
    fn content_counts(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for file in self.all_entries().into_iter().filter(|p| p.is_file()) {
            *counts.entry(hash_of(&file)).or_insert(0) += 1;
        }
        counts
    }

    /// Every file's path and content, plus every folder, as one comparable string. Staging folders
    /// are included: after a clean undo there should be none left.
    fn snapshot(&self) -> String {
        let mut lines = Vec::new();
        for entry in self.all_entries() {
            let relative = entry
                .strip_prefix(&self.root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if entry.is_file() {
                lines.push(format!("F {relative} {}", hash_of(&entry)));
            } else {
                lines.push(format!("D {relative}"));
            }
        }
        lines.sort();
        lines.join("\n")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = DirectoryRemoval::remove_tree(&self.root.to_string_lossy());
    }
}

/// The content stands in for its digest: every file here is a short generated string, and what
/// matters is that two files compare equal exactly when their bytes do.
fn hash_of(path: &Path) -> String {
    String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned()
}

fn is_in_staging(path: &str) -> bool {
    path.contains(".bertbrowser-replaced-")
}

const SEEDS: [u64; 8] = [1, 2, 3, 5, 8, 13, 21, 34];

#[test]
fn a_sequence_of_moves_never_loses_any_file_content() {
    for seed in SEEDS {
        let f = Fixture::new();
        let mut rng = Rng::new(seed);
        f.build_tree(&mut rng);

        let expected = f.content_counts();
        assert!(!expected.is_empty(), "seed {seed}");

        for round in 0..8 {
            let resolution = resolution(&mut rng);
            f.perform_random_drop(&mut rng, TransferVerb::Move, resolution);

            // The multiset of file contents under the root is invariant under a move: every byte
            // that was there is still there, wherever it now lives (staging included).
            assert_eq!(expected, f.content_counts(), "seed {seed} round {round}");
        }
    }
}

#[test]
fn a_move_then_undo_restores_the_tree_exactly() {
    for seed in SEEDS {
        let f = Fixture::new();
        let mut rng = Rng::new(seed);
        f.build_tree(&mut rng);

        let before = f.snapshot();

        let resolution = resolution(&mut rng);
        let Some(outcome) = f.perform_random_drop(&mut rng, TransferVerb::Move, resolution) else {
            continue; // this seed produced no valid drop
        };
        if outcome.completed.is_empty() {
            continue;
        }

        assert_ne!(
            before,
            f.snapshot(),
            "seed {seed}: something really did move"
        );

        let undo = f.executor.undo(&outcome, &CancelToken::none());

        assert!(undo.failed.is_empty(), "seed {seed}: {:?}", undo.failed);
        assert_eq!(before, f.snapshot(), "seed {seed}");
    }
}

#[test]
fn a_copy_adds_without_disturbing_what_was_already_there() {
    for seed in [4u64, 7, 11] {
        let f = Fixture::new();
        let mut rng = Rng::new(seed);
        f.build_tree(&mut rng);

        let before = f.content_counts();
        let resolution = resolution(&mut rng);
        f.perform_random_drop(&mut rng, TransferVerb::Copy, resolution);
        let after = f.content_counts();

        // A copy is purely additive: every original content is still present, at least as often.
        for (hash, count) in &before {
            let now = after.get(hash).copied().unwrap_or(0);
            assert!(
                now >= *count,
                "seed {seed}: content {hash} went from {count} occurrences to {now}"
            );
        }
    }
}

/// Guards the guard: if the hash-multiset check could not tell a lost file from an intact tree,
/// every other assertion in this file would be worthless.
#[test]
fn the_content_invariant_actually_detects_a_lost_file() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.root.join("folder")).unwrap();
    std::fs::write(f.root.join("folder").join("a.txt"), "payload").unwrap();
    std::fs::write(f.root.join("b.txt"), "other").unwrap();

    let before = f.content_counts();
    std::fs::remove_file(f.root.join("folder").join("a.txt")).unwrap();

    assert_ne!(before, f.content_counts());
}

/// Same for the snapshot: a file moved to a different folder must show up as a change.
#[test]
fn the_snapshot_actually_detects_a_moved_file() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.root.join("folder")).unwrap();
    std::fs::write(f.root.join("a.txt"), "payload").unwrap();

    let before = f.snapshot();
    std::fs::rename(f.root.join("a.txt"), f.root.join("folder").join("a.txt")).unwrap();

    assert_ne!(before, f.snapshot());
}
