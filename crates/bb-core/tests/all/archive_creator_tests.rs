// Port of tests/BertBrowser.Core.Tests/ArchiveCreatorTests.cs.
use bb_core::archives::{
    ArchiveCreator, ArchiveFormats, ArchiveReader, ArchiveSource, ArchiveSourceWalk,
    ArchiveWriteFormat, ArchiveWriteRules, CompressionLevel, CompressionType, LibraryArchiveReader,
};
use bb_core::cancel::CancelToken;
use bb_core::transfer::TransferProgress;
use bb_test_support::archive_support::TempRoot;
use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, SetFileAttributesW};

struct Fixture {
    root: TempRoot,
    creator: ArchiveCreator,
    reader: LibraryArchiveReader,
}

impl Fixture {
    fn new() -> Fixture {
        Fixture {
            root: TempRoot::new("bertbrowser-create"),
            creator: ArchiveCreator::new(),
            reader: LibraryArchiveReader::new(),
        }
    }

    fn make(&self, relative: &str, body: &str) -> String {
        self.root.make(relative, body.as_bytes())
    }

    fn collect(&self, relative: &str, include_hidden: bool) -> Vec<ArchiveSource> {
        ArchiveSourceWalk::collect(
            &[self.root.path(relative)],
            include_hidden,
            &CancelToken::none(),
        )
        .unwrap()
    }

    fn create(
        &self,
        target: &str,
        format: ArchiveWriteFormat,
        level: CompressionLevel,
        sources: &[ArchiveSource],
    ) -> bb_core::archives::CreateArchiveOutcome {
        self.creator.create(
            target,
            format,
            level,
            sources,
            &CancelToken::none(),
            None,
            None,
        )
    }

    fn partials(&self) -> Vec<String> {
        std::fs::read_dir(&self.root.root)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(ArchiveCreator::PARTIAL_SUFFIX))
            .collect()
    }
}

fn set_hidden(path: &str) {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: a NUL-terminated path, valid for the call.
    assert_ne!(0, unsafe {
        SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_HIDDEN)
    });
}

/// The round trip: what went in comes back out, byte for byte.
#[test]
fn what_goes_in_comes_back_out() {
    for (format, suffix) in [
        (ArchiveWriteFormat::Zip, ".zip"),
        (ArchiveWriteFormat::Tar, ".tar"),
        (ArchiveWriteFormat::TarGz, ".tar.gz"),
        (ArchiveWriteFormat::TarBz2, ".tar.bz2"),
    ] {
        let f = Fixture::new();
        f.make("stuff/readme.txt", "hello");
        f.make("stuff/src/app.js", "code");

        let sources = f.collect("stuff", true);
        let target = f.root.path(&format!("out{suffix}"));

        let outcome = f.create(&target, format, CompressionLevel::Normal, &sources);

        assert!(
            outcome.failed.is_empty(),
            "{suffix}: {}",
            outcome.failed.join(" | ")
        );
        assert_eq!(2, outcome.files_written, "{suffix}");
        assert!(!outcome.cancelled, "{suffix}");
        assert!(std::path::Path::new(&target).exists(), "{suffix}");

        let index = f.reader.read(&target, None, &CancelToken::none());
        assert!(index.ok(), "{suffix}: {:?}", index.error);
        assert_eq!(2, index.file_count, "{suffix}");
        assert!(index.find("stuff\\readme.txt").is_some(), "{suffix}");
        assert!(index.find("stuff\\src\\app.js").is_some(), "{suffix}");

        let bytes = f.reader.read_entry(
            &target,
            "stuff\\readme.txt",
            1024,
            None,
            &CancelToken::none(),
        );
        assert_eq!(Some(b"hello".to_vec()), bytes, "{suffix}");
    }
}

/// The three words in the dialog have to mean three things. Store and Deflate were the only two
/// the writer ever saw for a while, so Normal and Maximum produced byte-identical archives.
#[test]
fn maximum_asks_the_deflater_for_more_than_normal_does() {
    let normal = ArchiveCreator::options_for(ArchiveWriteFormat::Zip, CompressionLevel::Normal);
    let maximum = ArchiveCreator::options_for(ArchiveWriteFormat::Zip, CompressionLevel::Maximum);

    assert_eq!(CompressionType::Deflate, normal.compression_type);
    assert_eq!(CompressionType::Deflate, maximum.compression_type);
    assert_eq!(9, maximum.compression_level); // deflate's best
    assert!(
        maximum.compression_level > normal.compression_level,
        "Maximum has to try harder than Normal, or the dialog is offering a distinction that does not exist."
    );
}

/// The same thing measured: on compressible data, Maximum is never larger than Normal, and Store
/// is larger than both.
#[test]
fn maximum_is_no_larger_than_normal_and_store_is_larger_than_both() {
    let f = Fixture::new();
    // A fixed-seed linear congruential generator, so the corpus is the same on every run.
    let mut state = 7u64;
    let mut next = |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % bound
    };
    let words = [
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
    ];
    let mut text = String::new();
    for _ in 0..60_000 {
        text.push_str(words[next(words.len() as u64) as usize]);
        text.push_str(&next(100).to_string());
        text.push(' ');
    }
    f.make("stuff/corpus.txt", &text);
    let sources = f.collect("stuff", true);

    let size_at = |level: CompressionLevel, name: &str| -> u64 {
        let target = f.root.path(name);
        f.create(&target, ArchiveWriteFormat::Zip, level, &sources);
        std::fs::metadata(&target).unwrap().len()
    };

    let store = size_at(CompressionLevel::Store, "store.zip");
    let normal = size_at(CompressionLevel::Normal, "normal.zip");
    let maximum = size_at(CompressionLevel::Maximum, "maximum.zip");

    assert!(normal < store);
    assert!(maximum <= normal);
}

/// A stored zip is still readable: it is the right answer for already-compressed data.
#[test]
fn store_produces_a_readable_archive() {
    let f = Fixture::new();
    f.make("stuff/a.txt", "hello");
    let sources = f.collect("stuff", true);
    let target = f.root.path("stored.zip");

    f.create(
        &target,
        ArchiveWriteFormat::Zip,
        CompressionLevel::Store,
        &sources,
    );

    let index = f.reader.read(&target, None, &CancelToken::none());
    assert!(index.ok(), "{:?}", index.error);
    assert_eq!(1, index.file_count);
}

/// The reason it writes to a .bertbrowser-partial name first: a cancel must not leave a truncated
/// file under the name every other tool on the machine will try to open.
#[test]
fn a_cancelled_create_leaves_nothing_behind() {
    let f = Fixture::new();
    for i in 0..40 {
        f.make(&format!("stuff/f{i}.txt"), &"x".repeat(40_000));
    }
    let sources = f.collect("stuff", true);
    let target = f.root.path("cancelled.zip");

    let cancel = CancelToken::new();
    let trip = cancel.clone();
    let mut seen = 0;
    let mut progress = move |_: TransferProgress| {
        seen += 1;
        if seen > 3 {
            trip.cancel();
        }
    };

    let outcome = f.creator.create(
        &target,
        ArchiveWriteFormat::Zip,
        CompressionLevel::Normal,
        &sources,
        &cancel,
        Some(&mut progress),
        None,
    );

    assert!(outcome.cancelled);
    assert!(!std::path::Path::new(&target).exists());
    assert!(!std::path::Path::new(&format!("{target}{}", ArchiveCreator::PARTIAL_SUFFIX)).exists());
    assert!(f.partials().is_empty());
}

/// The meta-test: the "nothing left behind" check can actually fail.
#[test]
fn the_leftover_check_notices_a_partial_file() {
    let f = Fixture::new();
    f.root
        .make(&format!("x.zip{}", ArchiveCreator::PARTIAL_SUFFIX), b"half");
    assert!(!f.partials().is_empty());
}

#[test]
fn progress_reaches_the_total_and_is_monotonic() {
    let f = Fixture::new();
    f.make("stuff/a.txt", &"x".repeat(20_000));
    f.make("stuff/b.txt", &"y".repeat(20_000));
    let sources = f.collect("stuff", true);

    let mut seen: Vec<i64> = Vec::new();
    let mut progress = |p: TransferProgress| seen.push(p.bytes_done);

    f.creator.create(
        &f.root.path("p.zip"),
        ArchiveWriteFormat::Zip,
        CompressionLevel::Normal,
        &sources,
        &CancelToken::none(),
        Some(&mut progress),
        None,
    );

    assert!(!seen.is_empty());
    let mut sorted = seen.clone();
    sorted.sort();
    assert_eq!(sorted, seen);
    assert_eq!(40_000, *seen.last().unwrap());
}

/// The browse setting decides what goes in, so the archive matches what was on show.
#[test]
fn hidden_files_follow_the_browse_setting() {
    let f = Fixture::new();
    f.make("stuff/visible.txt", "a");
    let hidden = f.make("stuff/secret.txt", "b");
    set_hidden(&hidden);

    let without = f.collect("stuff", false);
    let with = f.collect("stuff", true);

    assert_eq!(1, without.len());
    assert_eq!(2, with.len());
}

#[test]
fn a_selected_folder_keeps_its_own_name_at_the_top() {
    let f = Fixture::new();
    f.make("stuff/deep/a.txt", "x");
    let sources = f.collect("stuff", true);

    assert_eq!(1, sources.len());
    assert_eq!("stuff/deep/a.txt", sources[0].entry_name);
}

#[test]
fn several_loose_files_go_in_side_by_side() {
    let f = Fixture::new();
    let a = f.make("a.txt", "1");
    let b = f.make("b.txt", "2");

    let sources = ArchiveSourceWalk::collect(&[a, b], true, &CancelToken::none()).unwrap();

    let mut names: Vec<String> = sources.iter().map(|s| s.entry_name.clone()).collect();
    names.sort();
    assert_eq!(vec!["a.txt", "b.txt"], names);
}

#[test]
fn formats_that_cannot_be_written_say_why_by_name() {
    for suffix in [".7z", ".rar", ".tar.xz"] {
        let why = ArchiveWriteRules::why_not_writable(Some(suffix));

        assert!(why.is_some(), "{suffix}");
        assert!(why.unwrap().to_lowercase().contains("read"), "{suffix}");
    }
}

#[test]
fn every_offered_format_really_produces_something_readable() {
    let f = Fixture::new();
    f.make("stuff/a.txt", "hello");
    let sources = f.collect("stuff", true);

    for info in ArchiveWriteRules::formats() {
        let target = f
            .root
            .path(&format!("each-{:?}{}", info.format, info.suffix));
        let outcome = f.create(&target, info.format, CompressionLevel::Normal, &sources);

        assert!(outcome.failed.is_empty(), "{}", info.label);
        assert!(
            f.reader.read(&target, None, &CancelToken::none()).ok(),
            "{} did not read back",
            info.label
        );
        // And the suffix it advertises is one the browse table recognises, or the archive it just
        // made could not be entered.
        assert!(
            ArchiveFormats::is_archive_name(&format!("x{}", info.suffix)),
            "{}",
            info.suffix
        );
    }
}
