//! The tool windows' engines: checksums (hash and verify), comparing and syncing two folders,
//! comparing two files (and the text diff behind it), the duplicate finder and disk usage. Split out
//! of bb-core; nothing in it is used there.

pub mod checksums;
pub mod compare;
pub mod diff;
pub mod diskusage;
pub mod duplicates;
