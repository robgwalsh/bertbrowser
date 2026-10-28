//! One running digest over a stream of chunks. Spec: Core/Services/Checksums/DigestSink.cs.
//!
//! Exists because the four cryptographic algorithms and CRC-32 are different shapes of thing — the
//! first four are `digest::Digest` implementations, the last a `crc32fast::Hasher` with a
//! different finish and a different byte order. Hiding all five behind one enum is what lets the
//! file reader hold a list of digests and know nothing about which is which.

use sha2::Digest;

use super::ChecksumAlgorithm;

/// One algorithm's running state.
pub(crate) enum DigestSink {
    Md5(md5::Md5),
    Sha1(sha1::Sha1),
    Sha256(sha2::Sha256),
    Sha512(sha2::Sha512),
    Crc32(crc32fast::Hasher),
}

impl DigestSink {
    /// The one place an algorithm becomes a primitive.
    pub(crate) fn create(algorithm: ChecksumAlgorithm) -> DigestSink {
        match algorithm {
            ChecksumAlgorithm::Crc32 => DigestSink::Crc32(crc32fast::Hasher::new()),
            ChecksumAlgorithm::Md5 => DigestSink::Md5(md5::Md5::new()),
            ChecksumAlgorithm::Sha1 => DigestSink::Sha1(sha1::Sha1::new()),
            ChecksumAlgorithm::Sha256 => DigestSink::Sha256(sha2::Sha256::new()),
            ChecksumAlgorithm::Sha512 => DigestSink::Sha512(sha2::Sha512::new()),
        }
    }

    pub(crate) fn append(&mut self, data: &[u8]) {
        match self {
            DigestSink::Md5(h) => h.update(data),
            DigestSink::Sha1(h) => h.update(data),
            DigestSink::Sha256(h) => h.update(data),
            DigestSink::Sha512(h) => h.update(data),
            DigestSink::Crc32(h) => h.update(data),
        }
    }

    /// The digest as uppercase hex, no separators — the app's one digest format.
    ///
    /// **The CRC-32 byte order is the whole point of the last arm.** The C# library handed the
    /// checksum back *little-endian* and every .sfv file ever written states it big-endian — CRC-32
    /// of "123456789" is `CBF43926`, and the bytes came back as `2639F4CB`. Here the checksum is a
    /// `u32`, and `to_be_bytes` is the one spelling that renders it the way an SFV reads it. This
    /// is the most dangerous line in the checksum feature and the reason
    /// `FileDigesterTests::crc32_matches_the_known_vector` exists: getting it wrong produces a
    /// plausible eight-character digest that is wrong in a way no other test would notice, and
    /// every .sfv the app wrote would be quietly unverifiable anywhere else.
    pub(crate) fn finish(self) -> String {
        match self {
            DigestSink::Md5(h) => to_hex_upper(&h.finalize()),
            DigestSink::Sha1(h) => to_hex_upper(&h.finalize()),
            DigestSink::Sha256(h) => to_hex_upper(&h.finalize()),
            DigestSink::Sha512(h) => to_hex_upper(&h.finalize()),
            DigestSink::Crc32(h) => to_hex_upper(&h.finalize().to_be_bytes()),
        }
    }
}

/// `Convert.ToHexString`: uppercase, no separators.
pub fn to_hex_upper(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0x0F) as usize] as char);
    }
    out
}
