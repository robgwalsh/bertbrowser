// Port of tests/BertBrowser.Core.Tests/ChecksumFileTests.cs
use bb_core::checksums::{
    ChecksumAlgorithm, ChecksumAlgorithms, ChecksumFile, ChecksumFileProblemKind, ChecksumLine,
};

const SHA256: &str = "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855";
const MD5: &str = "D41D8CD98F00B204E9800998ECF8427E";
const CRC: &str = "CBF43926";

fn single<T>(items: &[T]) -> &T {
    assert_eq!(1, items.len(), "expected exactly one item");
    &items[0]
}

// --- the *sum family: digest first ---

#[test]
fn a_sum_line_is_digest_then_name() {
    let parse = ChecksumFile::parse(&format!("{SHA256}  setup.exe\n"), Some("list.sha256"));

    let line = single(&parse.lines);
    assert_eq!("setup.exe", line.name);
    assert_eq!(SHA256, line.digest);
    assert!(parse.problems.is_empty());
}

#[test]
fn the_binary_star_is_stripped_and_remembered() {
    let parse = ChecksumFile::parse(&format!("{SHA256} *setup.exe\n"), Some("list.sha256"));

    let line = single(&parse.lines);
    assert_eq!("setup.exe", line.name);
    assert!(line.binary);
}

#[test]
fn text_mode_has_no_star_and_is_not_binary() {
    let parse = ChecksumFile::parse(&format!("{SHA256}  notes.txt\n"), Some("list.sha256"));

    assert!(!single(&parse.lines).binary);
}

/// The digest never holds a space, so everything after the first run is the name — which is how
/// a file called "my holiday photos.zip" survives a round trip.
#[test]
fn a_sum_name_may_contain_spaces() {
    let parse = ChecksumFile::parse(
        &format!("{SHA256}  my holiday photos.zip\n"),
        Some("list.sha256"),
    );

    assert_eq!("my holiday photos.zip", single(&parse.lines).name);
}

// --- SFV: digest last ---

#[test]
fn an_sfv_line_is_name_then_digest() {
    let parse = ChecksumFile::parse(&format!("setup.exe {CRC}\n"), Some("list.sfv"));

    let line = single(&parse.lines);
    assert_eq!("setup.exe", line.name);
    assert_eq!(CRC, line.digest);
}

#[test]
fn an_sfv_name_may_contain_spaces() {
    let parse = ChecksumFile::parse(&format!("my holiday photos.zip {CRC}\n"), Some("list.sfv"));

    assert_eq!("my holiday photos.zip", single(&parse.lines).name);
}

/// The extension decides the order, not a sniff of the content. A file whose names happen to look
/// like digests is still an SFV if it is called one — guessing would make such a file unreadable
/// in a way its author could never diagnose.
#[test]
fn the_extension_decides_the_order_not_the_content() {
    let parse = ChecksumFile::parse(&format!("{MD5} {CRC}\n"), Some("list.sfv"));

    let line = single(&parse.lines);
    assert_eq!(MD5, line.name);
    assert_eq!(CRC, line.digest);
}

// --- an unlabelled file ---

#[test]
fn an_unlabelled_file_is_sniffed_as_digest_first() {
    let parse = ChecksumFile::parse(&format!("{SHA256}  setup.exe\n"), Some("CHECKSUMS.txt"));

    assert_eq!("setup.exe", single(&parse.lines).name);
    assert_eq!(Some(ChecksumAlgorithm::Sha256), parse.algorithm);
}

#[test]
fn an_unlabelled_file_with_trailing_digests_is_sniffed_as_sfv() {
    let parse = ChecksumFile::parse(&format!("setup.exe {CRC}\n"), Some("CHECKSUMS.txt"));

    assert_eq!("setup.exe", single(&parse.lines).name);
    assert_eq!(Some(ChecksumAlgorithm::Crc32), parse.algorithm);
}

// --- comments, blanks, line endings ---

#[test]
fn comments_and_blanks_are_skipped_in_every_format() {
    let text = format!("; a comment\n\n# another\n{SHA256}  setup.exe\n\n");

    let parse = ChecksumFile::parse(&text, Some("list.sha256"));

    assert_eq!(1, parse.lines.len());
    assert!(parse.problems.is_empty());
}

#[test]
fn cr_lf_is_handled() {
    let parse = ChecksumFile::parse(
        &format!("{SHA256}  a.bin\r\n{MD5}  b.bin\r\n"),
        Some("list.txt"),
    );

    assert_eq!(2, parse.lines.len());
}

// --- problems are collected, never thrown ---

#[test]
fn a_malformed_line_is_one_problem_and_its_neighbours_still_parse() {
    let text = format!("{SHA256}  a.bin\nthis is not a checksum line\n{SHA256}  c.bin\n");

    let parse = ChecksumFile::parse(&text, Some("list.sha256"));

    assert_eq!(2, parse.lines.len());
    let problem = single(&parse.problems);
    assert_eq!(ChecksumFileProblemKind::Malformed, problem.kind);
    assert_eq!(2, problem.line_number);
}

#[test]
fn a_digest_of_no_known_length_is_malformed() {
    let parse = ChecksumFile::parse("ABCD  a.bin\n", Some("list.sha256"));

    assert!(parse.lines.is_empty());
    assert_eq!(
        ChecksumFileProblemKind::Malformed,
        single(&parse.problems).kind
    );
}

#[test]
fn a_duplicate_name_is_a_problem_not_two_rows() {
    let padded = format!("{MD5:A<64}");
    let parse = ChecksumFile::parse(
        &format!("{SHA256}  a.bin\n{padded}  a.bin\n"),
        Some("list.sha256"),
    );

    assert_eq!(1, parse.lines.len());
    assert_eq!(
        ChecksumFileProblemKind::DuplicateName,
        single(&parse.problems).kind
    );
}

// --- algorithm inference ---

#[test]
fn the_digest_length_overrides_a_misleading_extension() {
    // Named .md5, but every digest is 64 characters. The digests are the fact.
    let parse = ChecksumFile::parse(&format!("{SHA256}  a.bin\n"), Some("list.md5"));

    assert_eq!(Some(ChecksumAlgorithm::Sha256), parse.algorithm);
}

#[test]
fn digests_of_different_lengths_leave_the_algorithm_unknown() {
    let parse = ChecksumFile::parse(
        &format!("{SHA256}  a.bin\n{MD5}  b.bin\n"),
        Some("list.txt"),
    );

    assert_eq!(None, parse.algorithm);
    assert_eq!(2, parse.lines.len());
}

// --- rendering ---

/// The one casing rule in the app, and the only place it is applied: sha256sum writes lower case,
/// SFV writes upper. Internally every digest is upper, so this is purely about the file.
#[test]
fn sum_files_are_lowercase_and_sfv_is_uppercase() {
    let sum = ChecksumFile::render(
        ChecksumAlgorithm::Sha256,
        &[ChecksumLine::new("a.bin", SHA256)],
    );
    let sfv = ChecksumFile::render(
        ChecksumAlgorithm::Crc32,
        &[ChecksumLine::new("a.bin", &CRC.to_lowercase())],
    );

    assert!(sum.contains(&SHA256.to_lowercase()));
    assert!(sfv.contains(CRC));
}

#[test]
fn a_sum_file_renders_as_sha_sum_writes() {
    assert_eq!(
        format!("{}  *setup.exe\n", SHA256.to_lowercase()),
        ChecksumFile::render(
            ChecksumAlgorithm::Sha256,
            &[ChecksumLine::new("setup.exe", SHA256)]
        )
    );
}

#[test]
fn an_sfv_file_carries_its_comment_header() {
    assert!(
        ChecksumFile::render(ChecksumAlgorithm::Crc32, &[ChecksumLine::new("a.bin", CRC)])
            .starts_with(';')
    );
}

#[test]
fn render_round_trips_through_parse() {
    for (algorithm, digest) in [
        (ChecksumAlgorithm::Crc32, CRC),
        (ChecksumAlgorithm::Md5, MD5),
        (ChecksumAlgorithm::Sha256, SHA256),
    ] {
        let written = ChecksumLine::new("some folder/my file.zip", digest);

        let text = ChecksumFile::render(algorithm, std::slice::from_ref(&written));
        let parse = ChecksumFile::parse(
            &text,
            Some(&format!(
                "list{}",
                ChecksumAlgorithms::file_extension(algorithm)
            )),
        );

        let read = single(&parse.lines);
        assert_eq!(written.name, read.name, "{algorithm:?}");
        assert_eq!(
            digest.to_uppercase(),
            read.digest.to_uppercase(),
            "{algorithm:?}"
        );
        assert_eq!(Some(algorithm), parse.algorithm, "{algorithm:?}");
        assert!(parse.problems.is_empty(), "{algorithm:?}");
    }
}
