//! `--selftest`: the whole index pipeline over the in-memory fake volume, into a scratch database
//! under %TEMP%, with no token and no pipe. What it proves: the boot sector and FILE records parse,
//! the roll-up and the path builder agree with the records, the service builds `fs_entry` and
//! `dir_size_cache` for a volume, and the tail applies a create and a delete from canned USN
//! batches. Exit 0 on success; every failed check is printed.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bb_core::FileTime;
use bb_core::attributes::ARCHIVE;
use bb_core::cancel::CancelToken;
use bb_core::data::{Db, DirSizeRepository, FsIndexRepository};
use bb_core::mft::fixtures::{
    FakeVolume, FakeVolumeAccess, FileRecordSpec, VolumeImageBuilder, build_usn_record,
};
use bb_core::mft::{MftIndex, MftIndexService, usn_reason};
use bb_paths::PathKey;

const CLOSE_CREATE: u32 = usn_reason::CLOSE | usn_reason::FILE_CREATE;
const CLOSE_DELETE: u32 = usn_reason::CLOSE | usn_reason::FILE_DELETE;

pub fn run() -> i32 {
    let mut failures = Vec::new();
    let mut check = |name: &str, ok: bool| {
        println!("{} {name}", if ok { "ok  " } else { "FAIL" });
        if !ok {
            failures.push(name.to_string());
        }
    };

    let db_path =
        std::env::temp_dir().join(format!("bb-indexer-selftest-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db_path);
    let db = Arc::new(Db::new(db_path.clone()));
    if let Err(e) = db.migrate() {
        println!("FAIL migrate: {e}");
        return 1;
    }
    let repository = Arc::new(FsIndexRepository::new(db.clone()));
    let dir_sizes = Arc::new(DirSizeRepository::new(db.clone()));

    // Q:\Data\{a.txt=100, b.txt=200}; Q:\Data\Inner\c.txt=50; Q:\top.txt=7
    let mut image = VolumeImageBuilder::new();
    image
        .add(20, FileRecordSpec::dir("Data", 5))
        .add(21, FileRecordSpec::dir("Inner", 20))
        .add(30, FileRecordSpec::file("a.txt", 20, 100))
        .add(31, FileRecordSpec::file("b.txt", 20, 200))
        .add(32, FileRecordSpec::file("c.txt", 21, 50))
        .add(33, FileRecordSpec::file("top.txt", 5, 7));
    let mut volume = FakeVolume::new(image.build());
    // One drained batch: a file created under Data, then top.txt deleted.
    volume.push_batch(
        200,
        &[
            build_usn_record(
                40,
                20,
                CLOSE_CREATE,
                ARCHIVE,
                "new.txt",
                2,
                150,
                FileTime::now().0,
            ),
            build_usn_record(
                33,
                5,
                CLOSE_DELETE,
                ARCHIVE,
                "top.txt",
                2,
                160,
                FileTime::now().0,
            ),
        ],
    );
    let access = FakeVolumeAccess::new();
    access.add("Q", volume);

    let service = MftIndexService::new(
        repository.clone(),
        dir_sizes.clone(),
        Arc::new(access),
        None,
    );
    check("service starts", service.start().is_ok());

    let deadline = Instant::now() + Duration::from_secs(10);
    while !service.any_indexed() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    check("Q:\\ reported complete", service.is_indexed(r"Q:\DATA"));

    let root = repository.find_covering_root(r"Q:\").ok().flatten();
    check(
        "fs_index_root has Q:\\ complete",
        root.as_ref().is_some_and(|r| r.complete && !r.stale),
    );

    let data = dir_sizes.get(r"Q:\DATA").ok().flatten();
    check(
        "dir_size_cache Q:\\DATA = 350 bytes, 3 files, 1 dir",
        data.as_ref()
            .is_some_and(|d| d.size_bytes == 350 && d.file_count == 3 && d.dir_count == 1),
    );
    let inner = dir_sizes.get(r"Q:\DATA\INNER").ok().flatten();
    check(
        "dir_size_cache Q:\\DATA\\INNER = 50 bytes",
        inner.is_some_and(|d| d.size_bytes == 50),
    );

    let rows_under_root = || {
        repository
            .subtree(r"Q:\", true, 1000, None, &CancelToken::none())
            .map(|(rows, _)| rows)
            .unwrap_or_default()
    };

    // The tail drains its one batch on the first poll; give it a few.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut applied = false;
    while Instant::now() < deadline {
        let rows = rows_under_root();
        let has_new = rows.iter().any(|r| r.name == "new.txt");
        let has_top = rows.iter().any(|r| r.name == "top.txt");
        if has_new && !has_top {
            applied = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    check(
        "tail applied create of Q:\\Data\\new.txt and delete of Q:\\top.txt",
        applied,
    );

    service.shutdown();
    let rows = rows_under_root();
    check(
        "fs_entry holds Data, Inner, a, b, c, new (6 rows)",
        rows.len() == 6
            && rows.iter().filter(|r| r.is_directory).count() == 2
            && rows
                .iter()
                .any(|r| r.name == "a.txt" && r.size_bytes == 100),
    );
    check(
        "the keys are canonical",
        rows.iter().all(|r| {
            let key = format!("Q:\\{}", r.relative_key);
            PathKey::canonicalize(&key).is_ok_and(|c| c == key)
        }),
    );

    db.clear_pool();
    for suffix in ["", "-wal", "-shm"] {
        let mut p = db_path.as_os_str().to_owned();
        p.push(suffix);
        let _ = std::fs::remove_file(p);
    }

    if failures.is_empty() {
        println!("selftest passed");
        0
    } else {
        println!("selftest failed: {}", failures.join("; "));
        1
    }
}

/// The exe carries requireAdministrator, so an unelevated shell cannot even start it; the test
/// binary carries no manifest, which is how the self-test runs under `cargo test`.
#[cfg(test)]
mod tests {
    #[test]
    fn the_selftest_passes_without_a_token() {
        assert_eq!(0, super::run());
    }
}
