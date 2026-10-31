// Port of tests/BertBrowser.Core.Tests/ArchivePathTests.cs.
use bb_core::archives::ArchivePath;

/// Every path the tests below call an archive.
fn real_zips(p: &str) -> bool {
    p.eq_ignore_ascii_case(r"C:\stuff\a.zip")
        || p.eq_ignore_ascii_case(r"C:\stuff\backup.tar.gz")
        || p.eq_ignore_ascii_case(r"C:\stuff\outer.zip")
}

#[test]
fn splits_into_the_container_and_the_entry() {
    for (path, archive, entry) in [
        (r"C:\stuff\a.zip\src", r"C:\stuff\a.zip", "src"),
        (
            r"C:\stuff\a.zip\src\lib\util.js",
            r"C:\stuff\a.zip",
            r"src\lib\util.js",
        ),
        (
            r"C:\stuff\backup.tar.gz\etc",
            r"C:\stuff\backup.tar.gz",
            "etc",
        ),
    ] {
        let parsed = ArchivePath::parse(path, real_zips).unwrap_or_else(|| panic!("{path}"));
        assert_eq!(archive, parsed.archive_file);
        assert_eq!(entry, parsed.entry_path);
    }
}

#[test]
fn an_ordinary_folder_is_not_an_archive_path() {
    assert!(ArchivePath::parse(r"C:\Users\Rob\Documents", real_zips).is_none());
}

/// The reason existence is a probe rather than something the parser decides for itself: a folder
/// may perfectly well be named "photos.zip".
#[test]
fn a_folder_named_like_an_archive_is_not_one() {
    assert!(ArchivePath::parse(r"C:\stuff\photos.zip\holiday", |_| false).is_none());
}

/// The archive file itself parses, with an empty entry path.
#[test]
fn the_archive_itself_is_its_own_root() {
    let root = ArchivePath::new(r"C:\stuff\a.zip", "");

    assert!(root.is_root());
    assert!(root.parent().is_none());
    assert_eq!(r"C:\stuff\a.zip", root.to_string());
}

/// Zip Slip, spelled as a path rather than as an entry key. Resolving would turn this into
/// C:\Windows, so the refusal has to happen on the raw string.
#[test]
fn a_path_that_climbs_out_of_the_archive_is_refused() {
    for path in [
        r"C:\stuff\a.zip\..\..\Windows",
        r"C:\stuff\a.zip\src\..\..\..\Windows\System32",
        r"C:\stuff\a.zip\.\src",
    ] {
        assert!(!ArchivePath::is_acceptable(path), "{path}");
        assert!(ArchivePath::parse(path, real_zips).is_none(), "{path}");
    }
}

/// A zip inside a zip would have to be written out before it could be opened.
#[test]
fn a_nested_archive_is_refused_rather_than_entered() {
    assert!(ArchivePath::parse(r"C:\stuff\outer.zip\inner.zip", real_zips).is_none());
}

/// Shortest prefix wins: "outer.zip" is a file on disk and "outer.zip\inner.zip" is not.
#[test]
fn the_outermost_archive_wins() {
    let parsed = ArchivePath::parse(r"C:\stuff\outer.zip\inner.zip\notes.txt", real_zips).unwrap();
    assert_eq!(r"C:\stuff\outer.zip", parsed.archive_file);
    assert_eq!(r"inner.zip\notes.txt", parsed.entry_path);
}

#[test]
fn looks_virtual_is_a_pure_segment_scan() {
    for (path, expected) in [
        (r"C:\stuff\a.zip\src", true),
        (r"C:\stuff\backup.tar.gz\etc\hosts", true),
        (r"C:\Users\Rob\Documents", false),
        (r"C:\stuff\a.zip", true),
        ("", false),
    ] {
        assert_eq!(expected, ArchivePath::looks_virtual(path), "{path}");
    }
}

/// The archive file is its own root: an empty entry path, and a real folder above it.
#[test]
fn the_archive_file_parses_as_its_own_root() {
    let parsed = ArchivePath::parse(r"C:\stuff\a.zip", real_zips).unwrap();
    assert_eq!(r"C:\stuff\a.zip", parsed.archive_file);
    assert!(parsed.is_root());
    assert_eq!("", parsed.entry_path);
}

/// The navigation gate calls this on every path typed into the box, so it must never reach the disk
/// for an ordinary folder.
#[test]
fn looks_virtual_never_consults_the_probe() {
    let asked = std::cell::Cell::new(false);
    ArchivePath::parse(r"C:\Users\Rob\Documents", |_| {
        asked.set(true);
        true
    });

    assert!(!ArchivePath::looks_virtual(r"C:\Users\Rob\Documents"));
    assert!(!asked.get());
}

#[test]
fn parent_walks_up_and_then_stops() {
    let deep = ArchivePath::new(r"C:\stuff\a.zip", r"src\lib\util.js");

    let lib = deep.parent().unwrap();
    assert_eq!(r"src\lib", lib.entry_path);

    let src = lib.parent().unwrap();
    assert_eq!("src", src.entry_path);

    let root = src.parent().unwrap();
    assert!(root.is_root());
    assert!(root.parent().is_none());
}

#[test]
fn compose_round_trips() {
    for (archive, entry, expected) in [
        (r"C:\stuff\a.zip", "", r"C:\stuff\a.zip"),
        (r"C:\stuff\a.zip", "src", r"C:\stuff\a.zip\src"),
        (r"C:\stuff\a.zip", r"\src\lib\", r"C:\stuff\a.zip\src\lib"),
    ] {
        assert_eq!(expected, ArchivePath::compose(archive, entry));
    }
}

/// Deferred to NavigationRequest::is_acceptable_path rather than restated.
#[test]
fn unacceptable_paths_are_refused() {
    for path in [
        r"a.zip\src",
        r"\\.\PhysicalDrive0\x",
        "C:\\stuff\\a.zip\\sr\u{0}c",
        r"C:\stuff\a.zip\sr*c",
    ] {
        assert!(!ArchivePath::is_acceptable(path), "{path:?}");
    }
}
