// Port of tests/BertBrowser.Core.Tests/ExtractExecutorTests.cs.
use std::io::Read;
use std::sync::atomic::{AtomicI32, Ordering};

use bb_core::archives::{
    ArchiveIndex, ArchiveReadError, ArchiveReader, EntrySink, ExtractConflict, ExtractExecutor,
    ExtractOutcome, ExtractPlanner, LibraryArchiveReader,
};
use bb_core::cancel::{CancelToken, Cancelled};
use bb_core::transfer::TransferProgress;
use bb_test_support::archive_support::{TempRoot, write_zip};

struct Fixture {
    root: TempRoot,
    reader: LibraryArchiveReader,
    planner: ExtractPlanner,
}

impl Fixture {
    fn new() -> Fixture {
        let f = Fixture {
            root: TempRoot::new("bertbrowser-extract"),
            reader: LibraryArchiveReader::new(),
            planner: ExtractPlanner::default(),
        };
        std::fs::create_dir_all(f.dest()).unwrap();
        f
    }

    fn dest(&self) -> String {
        self.root.path("out")
    }

    fn in_dest(&self, relative: &str) -> String {
        format!("{}\\{}", self.dest(), relative.replace('/', "\\"))
    }

    fn zip(&self, name: &str, entries: &[(&str, &[u8])]) -> String {
        write_zip(&self.root.path(name), entries)
    }

    fn read(&self, archive: &str) -> ArchiveIndex {
        self.reader.read(archive, None, &CancelToken::none())
    }

    // The C# helper's optional parameters, spelled out.
    #[allow(clippy::too_many_arguments)]
    fn run(
        &self,
        archive: &str,
        index: &ArchiveIndex,
        relative_to: &str,
        entries: &[&str],
        conflict: ExtractConflict,
        cancel: &CancelToken,
        reader: Option<&dyn ArchiveReader>,
    ) -> ExtractOutcome {
        let entries: Vec<String> = entries.iter().map(|e| e.to_string()).collect();
        let plan = self.planner.plan(
            index,
            archive,
            relative_to,
            &entries,
            &self.dest(),
            conflict,
        );
        // Rejected, not has_work: a plan where every entry was skipped as a conflict has nothing
        // to write and is still a perfectly good plan.
        assert!(plan.rejected.is_none(), "{:?}", plan.rejected);
        ExtractExecutor::new(reader.unwrap_or(&self.reader))
            .execute(&plan, None, cancel, None, None)
    }

    fn run_all(&self, archive: &str, index: &ArchiveIndex) -> ExtractOutcome {
        self.run(
            archive,
            index,
            "",
            &[],
            ExtractConflict::KeepBoth,
            &CancelToken::none(),
            None,
        )
    }

    fn dest_entries(&self) -> Vec<String> {
        std::fs::read_dir(self.dest())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect()
    }
}

fn read_text(path: &str) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

/// Wraps the real reader and runs a hook between entries, so a cancel lands at a known point in
/// milliseconds: the stepped-copier pattern the transfer tests use.
struct SteppedReader<'a> {
    inner: &'a dyn ArchiveReader,
    after_entry: Box<dyn Fn(i32) + Send + Sync + 'a>,
}

impl ArchiveReader for SteppedReader<'_> {
    fn read(
        &self,
        archive_file: &str,
        password: Option<&str>,
        cancel: &CancelToken,
    ) -> ArchiveIndex {
        self.inner.read(archive_file, password, cancel)
    }

    fn read_entry(
        &self,
        archive_file: &str,
        entry_path: &str,
        max_bytes: i64,
        password: Option<&str>,
        cancel: &CancelToken,
    ) -> Option<Vec<u8>> {
        self.inner
            .read_entry(archive_file, entry_path, max_bytes, password, cancel)
    }

    fn read_entries(
        &self,
        archive_file: &str,
        entry_paths: &[String],
        password: Option<&str>,
        on_entry: EntrySink<'_>,
        cancel: &CancelToken,
    ) -> Result<(), ArchiveReadError> {
        let n = AtomicI32::new(0);
        let mut stepped = |key: &str, content: &mut dyn Read, size: i64| -> Result<(), Cancelled> {
            on_entry(key, content, size)?;
            (self.after_entry)(n.fetch_add(1, Ordering::SeqCst) + 1);
            cancel.check()
        };
        self.inner
            .read_entries(archive_file, entry_paths, password, &mut stepped, cancel)
    }
}

#[test]
fn extracts_every_file_with_its_contents() {
    let f = Fixture::new();
    let zip = f.zip(
        "a.zip",
        &[
            ("readme.txt", b"hello"),
            ("src/app.js", b"code"),
            ("src/lib/util.js", b"util"),
        ],
    );
    let index = f.read(&zip);

    let outcome = f.run_all(&zip, &index);

    assert_eq!(3, outcome.files_written);
    assert!(!outcome.cancelled);
    assert!(outcome.failed.is_empty());
    assert_eq!("hello", read_text(&f.in_dest("readme.txt")));
    assert_eq!("code", read_text(&f.in_dest("src/app.js")));
    assert_eq!("util", read_text(&f.in_dest("src/lib/util.js")));
}

#[test]
fn extracts_only_what_was_named() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("keep.txt", b"yes"), ("skip.txt", b"no")]);
    let index = f.read(&zip);

    f.run(
        &zip,
        &index,
        "",
        &["keep.txt"],
        ExtractConflict::KeepBoth,
        &CancelToken::none(),
        None,
    );

    assert!(exists(&f.in_dest("keep.txt")));
    assert!(!exists(&f.in_dest("skip.txt")));
}

/// Extracting a folder from inside it drops the prefix, so you get its contents.
#[test]
fn extracting_from_inside_a_folder_does_not_repeat_its_name() {
    let f = Fixture::new();
    let zip = f.zip(
        "a.zip",
        &[("src/app.js", b"code"), ("src/lib/util.js", b"util")],
    );
    let index = f.read(&zip);

    f.run(
        &zip,
        &index,
        "src",
        &[],
        ExtractConflict::KeepBoth,
        &CancelToken::none(),
        None,
    );

    assert!(exists(&f.in_dest("app.js")));
    assert!(exists(&f.in_dest("lib/util.js")));
    assert!(!exists(&f.in_dest("src")));
}

#[test]
fn an_empty_folder_in_the_archive_is_created() {
    let f = Fixture::new();
    let path = f.zip("e.zip", &[("outer/inner/deep.txt", b"x")]);

    let index = f.read(&path);
    f.run_all(&path, &index);

    assert!(std::path::Path::new(&f.in_dest("outer/inner")).is_dir());
}

#[test]
fn skip_leaves_what_is_already_there() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("readme.txt", b"from the archive")]);
    std::fs::write(f.in_dest("readme.txt"), "mine").unwrap();
    let index = f.read(&zip);

    let outcome = f.run(
        &zip,
        &index,
        "",
        &[],
        ExtractConflict::Skip,
        &CancelToken::none(),
        None,
    );

    assert_eq!(0, outcome.files_written);
    assert_eq!("mine", read_text(&f.in_dest("readme.txt")));
}

#[test]
fn keep_both_writes_beside_it() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("readme.txt", b"from the archive")]);
    std::fs::write(f.in_dest("readme.txt"), "mine").unwrap();
    let index = f.read(&zip);

    f.run(
        &zip,
        &index,
        "",
        &[],
        ExtractConflict::KeepBoth,
        &CancelToken::none(),
        None,
    );

    assert_eq!("mine", read_text(&f.in_dest("readme.txt")));
    assert_eq!("from the archive", read_text(&f.in_dest("readme (2).txt")));
}

/// The rule this whole type is shaped around. A cancelled extract removes exactly what it created,
/// and an extract routinely lands in a folder the user already has files in, so clearing the
/// destination tree the way a cancelled copy does would delete their work.
#[test]
fn a_cancelled_extract_leaves_the_users_own_files_in_the_destination() {
    let f = Fixture::new();
    let zip = f.zip(
        "a.zip",
        &[
            ("one.txt", b"1"),
            ("two.txt", b"2"),
            ("three.txt", b"3"),
            ("four.txt", b"4"),
        ],
    );
    let mine = f.in_dest("mine.txt");
    std::fs::write(&mine, "do not touch").unwrap();
    std::fs::create_dir_all(f.in_dest("myfolder")).unwrap();
    std::fs::write(f.in_dest("myfolder/deep.txt"), "also mine").unwrap();

    let index = f.read(&zip);
    let cancel = CancelToken::new();
    let trip = cancel.clone();
    let stepped = SteppedReader {
        inner: &f.reader,
        after_entry: Box::new(move |n| {
            if n >= 2 {
                trip.cancel();
            }
        }),
    };

    let outcome = f.run(
        &zip,
        &index,
        "",
        &[],
        ExtractConflict::KeepBoth,
        &cancel,
        Some(&stepped),
    );

    assert!(outcome.cancelled);
    assert_eq!("do not touch", read_text(&mine));
    assert_eq!("also mine", read_text(&f.in_dest("myfolder/deep.txt")));
}

/// And it takes back everything it did add, so a cancel is not half an extract.
#[test]
fn a_cancelled_extract_takes_back_what_it_added() {
    let f = Fixture::new();
    let zip = f.zip(
        "a.zip",
        &[
            ("d/one.txt", b"1"),
            ("d/two.txt", b"2"),
            ("d/three.txt", b"3"),
            ("d/four.txt", b"4"),
        ],
    );
    let index = f.read(&zip);

    let cancel = CancelToken::new();
    let trip = cancel.clone();
    let stepped = SteppedReader {
        inner: &f.reader,
        after_entry: Box::new(move |n| {
            if n >= 2 {
                trip.cancel();
            }
        }),
    };

    let outcome = f.run(
        &zip,
        &index,
        "",
        &[],
        ExtractConflict::KeepBoth,
        &cancel,
        Some(&stepped),
    );

    assert!(outcome.cancelled);
    assert!(!exists(&f.in_dest("d")));
    assert!(f.dest_entries().is_empty());
}

/// The meta-test: the check above can actually fail.
#[test]
fn the_empty_destination_check_notices_a_leftover_file() {
    let f = Fixture::new();
    std::fs::write(f.in_dest("leftover.txt"), "x").unwrap();
    assert!(!f.dest_entries().is_empty());
}

#[test]
fn byte_totals_are_exact_for_an_addressable_archive() {
    let f = Fixture::new();
    let zip = f.zip(
        "a.zip",
        &[
            ("a.txt", "x".repeat(100).as_bytes()),
            ("b.txt", "y".repeat(50).as_bytes()),
        ],
    );
    let index = f.read(&zip);

    let plan = f
        .planner
        .plan(&index, &zip, "", &[], &f.dest(), ExtractConflict::KeepBoth);

    assert!(plan.bytes_are_exact);
    assert_eq!(150, plan.total_bytes);

    let outcome =
        ExtractExecutor::new(&f.reader).execute(&plan, None, &CancelToken::none(), None, None);
    assert_eq!(150, outcome.bytes_written);
}

#[test]
fn progress_reaches_the_total_and_is_monotonic() {
    let f = Fixture::new();
    let zip = f.zip(
        "a.zip",
        &[
            ("a.txt", "x".repeat(5000).as_bytes()),
            ("b.txt", "y".repeat(5000).as_bytes()),
        ],
    );
    let index = f.read(&zip);
    let plan = f
        .planner
        .plan(&index, &zip, "", &[], &f.dest(), ExtractConflict::KeepBoth);

    let mut seen: Vec<i64> = Vec::new();
    let mut progress = |p: TransferProgress| seen.push(p.bytes_done);

    ExtractExecutor::new(&f.reader).execute(
        &plan,
        None,
        &CancelToken::none(),
        Some(&mut progress),
        None,
    );

    assert!(!seen.is_empty());
    let mut sorted = seen.clone();
    sorted.sort();
    assert_eq!(sorted, seen);
    assert_eq!(10000, *seen.last().unwrap());
}

/// Reports rather than fails, and never leaves the half-written file behind.
#[test]
fn an_unwritable_destination_is_reported_and_costs_the_others_nothing() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("ok.txt", b"fine"), ("blocked.txt", b"nope")]);
    // A directory where the file wants to be: creating the file fails, and nothing else cares.
    std::fs::create_dir_all(f.in_dest("blocked.txt")).unwrap();

    let index = f.read(&zip);
    let outcome = f.run(
        &zip,
        &index,
        "",
        &[],
        ExtractConflict::Skip,
        &CancelToken::none(),
        None,
    );

    assert_eq!("fine", read_text(&f.in_dest("ok.txt")));
    assert_eq!(1, outcome.files_written);
}
