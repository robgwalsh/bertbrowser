//! `--dump-stats`: opens every fixed NTFS volume raw, queries its journal and reads the whole
//! `$MFT`, printing counts and timings. Writes nothing. Needs an administrator token, which is
//! what makes it the one way to exercise `RawVolume` and `MftReader` against real disks: run it
//! elevated, by hand.

use std::time::Instant;

use bb_core::cancel::CancelToken;
use bb_core::mft::{MftReader, RawVolumeAccess, VolumeAccess, VolumeHandle};

pub fn run() -> i32 {
    let access = RawVolumeAccess;
    let volumes = access.ntfs_volumes();
    if volumes.is_empty() {
        println!("no fixed NTFS volumes found");
        return 0;
    }
    let mut code = 0;
    for letter in volumes {
        print!("{letter}: ");
        let Some(mut handle) = access.open(&letter) else {
            println!("could not open \\\\.\\{letter}: (run elevated)");
            code = 1;
            continue;
        };
        match handle.query_journal() {
            Some(journal) => println!(
                "journal id {:#x}, next USN {}, max USN {}",
                journal.journal_id, journal.next_usn, journal.max_usn
            ),
            None => {
                println!("no USN journal could be queried or created");
                code = 1;
            }
        }

        let started = Instant::now();
        let (mut files, mut dirs, mut bytes, mut unknown_sizes) = (0u64, 0u64, 0i64, 0u64);
        let handle: &dyn VolumeHandle = handle.as_ref();
        let reader = MftReader::new(handle);
        let outcome = reader.try_read_all(&CancelToken::none(), |rec| {
            if rec.is_directory {
                dirs += 1;
            } else {
                files += 1;
                if rec.size < 0 {
                    unknown_sizes += 1;
                } else {
                    bytes += rec.size;
                }
            }
        });
        match outcome {
            Ok(true) => println!(
                "    $MFT: {files} files, {dirs} directories, {bytes} bytes, {unknown_sizes} sizes needing a stat, in {:.2?}",
                started.elapsed()
            ),
            Ok(false) => {
                println!(
                    "    boot sector or $MFT runlist could not be parsed (names-only fallback would be used)"
                );
                code = 1;
            }
            Err(e) => {
                println!("    read failed: {e:?}");
                code = 1;
            }
        }
    }
    code
}
