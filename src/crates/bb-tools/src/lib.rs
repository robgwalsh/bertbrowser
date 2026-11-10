//! The tool windows' engines: checksums (hash and verify), the duplicate finder and disk usage. Split out
//! of bb-core; nothing in it is used there.

pub mod checksums;
pub mod diskusage;
pub mod duplicates;
