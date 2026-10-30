// Port of tests/BertBrowser.Core.Tests/ArchiveReaderTests.cs. The reader against real containers.
// The fixtures are written with this crate's own writer, which makes most of these round-trip
// tests rather than compatibility tests. What they do pin, and what no round trip could fake, is
// the failure behaviour: damaged bytes and a file that only claims to be an archive all have to
// come back as an index carrying a message, because the whole browsing surface rests on that
// never failing.
use bb_core::archives::{
    ArchiveFailure, ArchiveReader, ArchiveType, CompressionType, LibraryArchiveReader,
};
use bb_core::cancel::CancelToken;
use bb_test_support::archive_support::{TempRoot, write_archive, write_zip};

struct Fixture {
    root: TempRoot,
    reader: LibraryArchiveReader,
}

impl Fixture {
    fn new() -> Fixture {
        Fixture {
            root: TempRoot::new("bertbrowser-archread"),
            reader: LibraryArchiveReader::new(),
        }
    }

    fn write(
        &self,
        kind: ArchiveType,
        compression: CompressionType,
        name: &str,
        entries: &[(&str, &[u8])],
    ) -> String {
        write_archive(&self.root.path(name), kind, compression, entries)
    }

    fn zip(&self, name: &str, entries: &[(&str, &[u8])]) -> String {
        write_zip(&self.root.path(name), entries)
    }

    fn read(&self, path: &str) -> bb_core::archives::ArchiveIndex {
        self.reader.read(path, None, &CancelToken::none())
    }

    fn entry(&self, path: &str, entry: &str, max: i64) -> Option<Vec<u8>> {
        self.reader
            .read_entry(path, entry, max, None, &CancelToken::none())
    }
}

fn names(index: &bb_core::archives::ArchiveIndex, dir: &str) -> Vec<String> {
    index
        .children(dir)
        .expect("directory")
        .iter()
        .map(|n| n.name.clone())
        .collect()
}

#[test]
fn reads_a_zip_into_a_tree() {
    let f = Fixture::new();
    let path = f.zip(
        "a.zip",
        &[
            ("readme.txt", b"hello"),
            ("src/app.js", b"console.log(1)"),
            ("src/lib/util.js", b"export {}"),
        ],
    );

    let index = f.read(&path);

    assert!(index.ok());
    assert_eq!(3, index.file_count);
    assert_eq!(vec!["readme.txt", "src"], names(&index, ""));
    assert_eq!(vec!["app.js", "lib"], names(&index, "src"));
    assert_eq!(
        "export {}".len() as i64,
        index.find("src\\lib\\util.js").unwrap().size_bytes
    );
}

#[test]
fn folders_get_their_exact_recursive_size() {
    let f = Fixture::new();
    let path = f.zip(
        "sizes.zip",
        &[
            ("d/a.txt", "x".repeat(100).as_bytes()),
            ("d/e/b.txt", "y".repeat(50).as_bytes()),
        ],
    );

    let index = f.read(&path);

    assert_eq!(150, index.find("d").unwrap().size_bytes);
    assert_eq!(50, index.find("d\\e").unwrap().size_bytes);
}

#[test]
fn reads_a_tar_gz_without_random_access() {
    let f = Fixture::new();
    let path = f.write(
        ArchiveType::Tar,
        CompressionType::GZip,
        "b.tar.gz",
        &[("etc/hosts", b"127.0.0.1"), ("etc/motd", b"hi")],
    );

    let index = f.read(&path);

    assert!(index.ok(), "{:?}", index.error);
    assert_eq!(2, index.file_count);
    assert_eq!(vec!["hosts", "motd"], names(&index, "etc"));
}

/// The format table decides which decoder to use rather than the reader guessing.
#[test]
fn reads_a_tar_bz2() {
    let f = Fixture::new();
    let path = f.write(
        ArchiveType::Tar,
        CompressionType::BZip2,
        "d.tar.bz2",
        &[("a/b.txt", b"body")],
    );

    let index = f.read(&path);

    assert!(index.ok(), "{:?}", index.error);
    assert_eq!(vec!["b.txt"], names(&index, "a"));
    assert!(index.capabilities.sequential_only);
}

/// A .tar.zst, written as two frames the way `pzstd` and a concatenation write one: both are read.
#[test]
fn reads_a_tar_zst_across_frames() {
    let f = Fixture::new();
    let tar = f.write(
        ArchiveType::Tar,
        CompressionType::None,
        "e.tar",
        &[("pkg/a.txt", b"first"), ("pkg/b.txt", b"second")],
    );
    let bytes = std::fs::read(&tar).unwrap();
    let (head, tail) = bytes.split_at(bytes.len() / 2);
    let level = ruzstd::encoding::CompressionLevel::Fastest;
    let mut packed = ruzstd::encoding::compress_to_vec(head, level);
    packed.extend(ruzstd::encoding::compress_to_vec(tail, level));
    let path = f.root.path("e.tar.zst");
    std::fs::write(&path, packed).unwrap();

    let index = f.read(&path);

    assert!(index.ok(), "{:?}", index.error);
    assert_eq!(vec!["a.txt", "b.txt"], names(&index, "pkg"));
    assert!(index.capabilities.sequential_only);
}

/// A gzipped single file carries the inner name, so syslog.gz browses to syslog.
#[test]
fn a_gzipped_file_browses_to_its_inner_name() {
    let f = Fixture::new();
    let path = f.write(
        ArchiveType::GZip,
        CompressionType::GZip,
        "syslog.gz",
        &[("syslog", b"line one")],
    );

    let index = f.read(&path);

    assert!(index.ok(), "{:?}", index.error);
    assert_eq!(vec!["syslog"], names(&index, ""));
}

/// A container that is not what its name claims is refused rather than browsed. Without this a
/// zip renamed to .7z would open and list, which is a quiet lie about what the file is.
#[test]
fn a_zip_wearing_another_suffix_is_refused() {
    let f = Fixture::new();
    let zip = f.zip("real.zip", &[("a.txt", b"x")]);
    let lying = f.root.path("real.7z");
    std::fs::rename(&zip, &lying).unwrap();

    let index = f.read(&lying);

    assert!(!index.ok());
    assert_eq!(ArchiveFailure::Damaged, index.failure);
}

#[test]
fn a_file_with_no_archive_suffix_is_never_opened() {
    let f = Fixture::new();
    let path = f.root.make("notes.txt", b"hello");

    let index = f.read(&path);

    assert!(!index.ok());
}

#[test]
fn reads_a_plain_tar() {
    let f = Fixture::new();
    let path = f.write(
        ArchiveType::Tar,
        CompressionType::None,
        "c.tar",
        &[("only.txt", b"x")],
    );

    let index = f.read(&path);

    assert!(index.ok(), "{:?}", index.error);
    assert_eq!(1, index.file_count);
}

/// The harness's ordinary fixture writes 512,000 bytes of filler and calls it archive.zip, and
/// three scripts list that folder today. Anything that parses archives has to survive it.
#[test]
fn a_file_that_only_claims_to_be_an_archive_is_a_message_rather_than_a_throw() {
    let f = Fixture::new();
    let path = f.root.make("archive.zip", &vec![0u8; 512_000]);

    let index = f.read(&path);

    assert!(!index.ok());
    assert_eq!(ArchiveFailure::Damaged, index.failure);
    assert!(index.error.is_some());
}

#[test]
fn a_truncated_archive_is_a_message_rather_than_a_throw() {
    let f = Fixture::new();
    let path = f.zip("torn.zip", &[("a.txt", "q".repeat(5000).as_bytes())]);
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes[..bytes.len() / 3]).unwrap();

    let index = f.read(&path);

    assert!(!index.ok());
    assert!(index.error.is_some());
}

#[test]
fn a_missing_archive_is_a_message_rather_than_a_throw() {
    let f = Fixture::new();

    let index = f.read(&f.root.path("nope.zip"));

    assert!(!index.ok());
    assert_eq!(ArchiveFailure::Unreadable, index.failure);
}

#[test]
fn an_empty_archive_reads_as_empty_rather_than_broken() {
    let f = Fixture::new();
    let path = f.zip("empty.zip", &[]);

    let index = f.read(&path);

    assert!(index.ok(), "{:?}", index.error);
    assert_eq!(0, index.file_count);
    assert!(index.children("").unwrap().is_empty());
}

#[test]
fn reads_one_entrys_bytes() {
    let f = Fixture::new();
    let path = f.zip("read.zip", &[("src/app.js", b"console.log(1)")]);

    let bytes = f.entry(&path, "src\\app.js", 1024);

    assert_eq!(Some(b"console.log(1)".to_vec()), bytes);
}

/// The budget is what makes a decompression bomb harmless: pulling a megabyte out of a stream
/// that would have produced ten gigabytes costs a megabyte.
#[test]
fn reading_an_entry_stops_at_the_budget() {
    let f = Fixture::new();
    let path = f.zip("big.zip", &[("huge.txt", "z".repeat(200_000).as_bytes())]);

    let bytes = f.entry(&path, "huge.txt", 4096).expect("bytes");

    assert_eq!(4096, bytes.len());
}

#[test]
fn reading_an_entry_that_is_not_there_is_null() {
    let f = Fixture::new();
    let path = f.zip("read.zip", &[("a.txt", b"x")]);

    assert!(f.entry(&path, "missing.txt", 1024).is_none());
}

/// The point of the sharing flags. This app's own rename, move and delete executors are what a
/// held handle would block, so browsing into an archive must not lock it.
#[test]
fn reading_does_not_hold_the_archive_open() {
    let f = Fixture::new();
    let path = f.zip("free.zip", &[("a.txt", b"x")]);

    let index = f.read(&path);
    assert!(index.ok());
    f.entry(&path, "a.txt", 1024);

    let moved = f.root.path("moved.zip");
    std::fs::rename(&path, &moved).expect("rename would fail if a handle were still open");
    std::fs::remove_file(&moved).unwrap();

    assert!(!std::path::Path::new(&moved).exists());
}

#[test]
fn the_entry_cap_is_reported_rather_than_half_built() {
    let f = Fixture::new();
    let many: Vec<(String, Vec<u8>)> = (0..30)
        .map(|i| (format!("f{i}.txt"), b"x".to_vec()))
        .collect();
    let entries: Vec<(&str, &[u8])> = many
        .iter()
        .map(|(k, b)| (k.as_str(), b.as_slice()))
        .collect();
    let path = f.zip("many.zip", &entries);

    let index = LibraryArchiveReader::with_cap(10).read(&path, None, &CancelToken::none());

    assert!(!index.ok());
    assert_eq!(ArchiveFailure::TooManyEntries, index.failure);
}

/// Zip Slip through the real reader, not just the builder: a container really can carry this
/// key, and the entry must not exist to be extracted.
#[test]
fn an_entry_named_dot_dot_evil_never_reaches_the_index() {
    let f = Fixture::new();
    let path = f.zip(
        "slip.zip",
        &[("../../evil.dll", b"pwned"), ("safe.txt", b"ok")],
    );

    let index = f.read(&path);

    assert!(index.ok(), "{:?}", index.error);
    assert_eq!(1, index.file_count);
    assert_eq!(vec!["safe.txt"], names(&index, ""));
    assert!(index.refused_count >= 1);
    assert!(f.entry(&path, "evil.dll", 1024).is_none());
}
