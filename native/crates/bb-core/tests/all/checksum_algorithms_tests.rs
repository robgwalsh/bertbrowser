// Port of tests/BertBrowser.Core.Tests/ChecksumAlgorithmsTests.cs
use std::collections::HashSet;

use bb_core::checksums::{ChecksumAlgorithm, ChecksumAlgorithms};
use sha2::Digest;

#[test]
fn every_algorithm_is_in_the_catalogue() {
    // The enum has five variants; the table lists each of them once.
    let distinct: HashSet<ChecksumAlgorithm> = ChecksumAlgorithms::ALL.into_iter().collect();
    assert_eq!(5, ChecksumAlgorithms::ALL.len());
    assert_eq!(5, distinct.len());
}

/// The lengths are computed from the primitives rather than restated, so the table cannot drift
/// from what the algorithms actually produce.
#[test]
fn hex_length_is_twice_the_digest_size() {
    for (algorithm, digest_bytes) in [
        (ChecksumAlgorithm::Md5, 16),
        (ChecksumAlgorithm::Sha1, 20),
        (ChecksumAlgorithm::Sha256, 32),
        (ChecksumAlgorithm::Sha512, 64),
        (ChecksumAlgorithm::Crc32, 4),
    ] {
        assert_eq!(
            digest_bytes * 2,
            ChecksumAlgorithms::hex_length(algorithm),
            "{algorithm:?}"
        );
    }
}

#[test]
fn the_cryptographic_lengths_agree_with_the_bcl() {
    assert_eq!(
        md5::Md5::output_size() * 2,
        ChecksumAlgorithms::hex_length(ChecksumAlgorithm::Md5)
    );
    assert_eq!(
        sha1::Sha1::output_size() * 2,
        ChecksumAlgorithms::hex_length(ChecksumAlgorithm::Sha1)
    );
    assert_eq!(
        sha2::Sha256::output_size() * 2,
        ChecksumAlgorithms::hex_length(ChecksumAlgorithm::Sha256)
    );
    assert_eq!(
        sha2::Sha512::output_size() * 2,
        ChecksumAlgorithms::hex_length(ChecksumAlgorithm::Sha512)
    );
}

/// No two algorithms share a digest length. This is what makes `from_digest_length` possible at
/// all, and so what lets the compare box work out for itself which algorithm someone pasted.
#[test]
fn no_two_algorithms_share_a_digest_length() {
    let lengths: HashSet<usize> = ChecksumAlgorithms::ALL
        .into_iter()
        .map(ChecksumAlgorithms::hex_length)
        .collect();
    assert_eq!(ChecksumAlgorithms::ALL.len(), lengths.len());
}

#[test]
fn no_two_algorithms_share_an_extension() {
    let extensions: HashSet<String> = ChecksumAlgorithms::ALL
        .into_iter()
        .map(|a| ChecksumAlgorithms::file_extension(a).to_ascii_lowercase())
        .collect();
    assert_eq!(ChecksumAlgorithms::ALL.len(), extensions.len());
}

#[test]
fn every_extension_round_trips() {
    for algorithm in ChecksumAlgorithms::ALL {
        let extension = ChecksumAlgorithms::file_extension(algorithm);
        assert_eq!(
            Some(algorithm),
            ChecksumAlgorithms::from_extension(Some(extension))
        );
        assert_eq!(
            Some(algorithm),
            ChecksumAlgorithms::from_extension(Some(&format!("list{extension}")))
        );
    }
}

#[test]
fn every_digest_length_round_trips() {
    for algorithm in ChecksumAlgorithms::ALL {
        assert_eq!(
            Some(algorithm),
            ChecksumAlgorithms::from_digest_length(ChecksumAlgorithms::hex_length(algorithm))
        );
    }
}

#[test]
fn an_unknown_extension_is_null() {
    assert_eq!(None, ChecksumAlgorithms::from_extension(Some("notes.txt")));
    assert_eq!(None, ChecksumAlgorithms::from_extension(Some("")));
    assert_eq!(None, ChecksumAlgorithms::from_extension(None));
}

#[test]
fn an_unknown_digest_length_is_null() {
    assert_eq!(None, ChecksumAlgorithms::from_digest_length(63));
    assert_eq!(None, ChecksumAlgorithms::from_digest_length(0));
}

#[test]
fn recognise_needs_both_hex_and_a_known_length() {
    assert_eq!(
        Some(ChecksumAlgorithm::Crc32),
        ChecksumAlgorithms::recognise("CBF43926")
    );
    assert_eq!(
        Some(ChecksumAlgorithm::Crc32),
        ChecksumAlgorithms::recognise("cbf43926")
    );

    assert_eq!(None, ChecksumAlgorithms::recognise("CBF4392Z")); // not hex
    assert_eq!(None, ChecksumAlgorithms::recognise("CBF4392")); // not a known length
    assert_eq!(None, ChecksumAlgorithms::recognise(""));
}

/// Normalising here rather than at each call site is what stops two windows disagreeing about
/// the order they show digests in.
#[test]
fn normalise_deduplicates_and_orders() {
    let normalised = ChecksumAlgorithms::normalise([
        ChecksumAlgorithm::Sha512,
        ChecksumAlgorithm::Md5,
        ChecksumAlgorithm::Sha512,
    ]);

    assert_eq!(
        vec![ChecksumAlgorithm::Md5, ChecksumAlgorithm::Sha512],
        normalised
    );
}

#[test]
fn normalise_of_nothing_is_nothing() {
    assert!(ChecksumAlgorithms::normalise([]).is_empty());
}

/// Only SFV is written upper case; the *sum family is what people diff against.
#[test]
fn only_sfv_is_written_uppercase() {
    assert!(!ChecksumAlgorithms::is_written_lowercase(
        ChecksumAlgorithm::Crc32
    ));

    for algorithm in ChecksumAlgorithms::ALL
        .into_iter()
        .filter(|&a| a != ChecksumAlgorithm::Crc32)
    {
        assert!(
            ChecksumAlgorithms::is_written_lowercase(algorithm),
            "{algorithm:?}"
        );
    }
}
