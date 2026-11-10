// How a volume's build turns into one percentage: climbing, exact at the seams, stopping short of
// 100 while work remains, and taken back rather than left wrong when the build falls back to a path
// with no total.
use std::sync::Mutex;

use bb_base::cancel::CancelToken;
use bb_index::data::{DirSizeRepository, FsIndexRepository};
use bb_index::mft::fixtures::{FakeVolume, FakeVolumeAccess, FileRecordSpec, VolumeImageBuilder};
use bb_index::mft::{BuildPhase, BuildProgress, MftReader, MftVolumeIndexer};
use bb_test_support::TempDb;
use std::sync::Arc;

/// Every phase's whole range, in the order a build runs them.
fn every_percent() -> Vec<u8> {
    let mut all = Vec::new();
    for phase in [BuildPhase::Reading, BuildPhase::Writing, BuildPhase::Sizes] {
        for done in 0..=1000 {
            all.push(phase.percent(done, 1000));
        }
    }
    all
}

#[test]
fn the_number_only_ever_climbs_across_the_phases() {
    let all = every_percent();

    assert!(all.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(0, all[0]);
}

/// The drive leaves the status line when it is done; 100 would be a claim with work still to do.
#[test]
fn it_never_says_100() {
    assert_eq!(99, *every_percent().last().unwrap());
}

#[test]
fn each_phase_starts_where_the_last_ended() {
    assert_eq!(
        BuildPhase::Reading.percent(1, 1),
        BuildPhase::Writing.percent(0, 1)
    );
    assert_eq!(
        BuildPhase::Writing.percent(1, 1),
        BuildPhase::Sizes.percent(0, 1)
    );
}

/// An empty volume, or a run list that adds up to nothing, must not divide by zero.
#[test]
fn a_phase_with_nothing_to_do_is_already_over() {
    assert_eq!(
        BuildPhase::Writing.percent(1, 1),
        BuildPhase::Writing.percent(0, 0)
    );
}

#[test]
fn more_done_than_the_total_stays_within_the_phase() {
    assert_eq!(
        BuildPhase::Reading.percent(1, 1),
        BuildPhase::Reading.percent(5, 1)
    );
}

/// A message crosses the pipe and repaints the window per report, so a million calls that land on
/// one percentage must be one report.
#[test]
fn reports_each_percentage_once() {
    let heard = Mutex::new(Vec::new());
    let report = |percent: Option<u8>| heard.lock().unwrap().push(percent);
    let mut progress = BuildProgress::new(&report);

    for done in 0..=10_000 {
        progress.at(BuildPhase::Reading, done, 10_000);
    }

    let heard = heard.lock().unwrap();
    let end = BuildPhase::Reading.percent(1, 1);
    assert_eq!(usize::from(end) + 1, heard.len());
    assert_eq!(Some(end), *heard.last().unwrap());
}

/// Nothing to take back means nothing to say: a build that fell back before its first chunk.
#[test]
fn unknown_is_silent_until_a_number_has_been_given() {
    let heard = Mutex::new(Vec::new());
    let report = |percent: Option<u8>| heard.lock().unwrap().push(percent);
    let mut progress = BuildProgress::new(&report);

    progress.unknown();
    assert!(heard.lock().unwrap().is_empty());

    progress.at(BuildPhase::Reading, 1, 2);
    progress.unknown();
    let halfway = BuildPhase::Reading.percent(1, 2);
    assert_eq!(vec![Some(halfway), None], *heard.lock().unwrap());
}

fn image() -> Vec<u8> {
    let mut image = VolumeImageBuilder::new();
    image
        .add(20, FileRecordSpec::dir("Data", 5))
        .add(30, FileRecordSpec::file("a.txt", 20, 100))
        .add(5000, FileRecordSpec::file("far.txt", 20, 7));
    image.build()
}

/// The total has to be known from the run list, before the first chunk, and the last report has to
/// land on it: a run list misread as shorter than the table would climb past the end.
#[test]
fn the_reader_reports_bytes_against_a_total_known_from_the_start() {
    let volume = FakeVolume::new(image());
    let heard = Mutex::new(Vec::new());

    let read = MftReader::new(&volume).try_read_all_reporting(
        &CancelToken::none(),
        |_| {},
        |done, total| heard.lock().unwrap().push((done, total)),
    );

    assert!(matches!(read, Ok(true)));
    let heard = heard.lock().unwrap();
    let total = heard[0].1;
    assert!(heard.iter().all(|(_, t)| *t == total));
    assert!(heard.windows(2).all(|pair| pair[0].0 < pair[1].0));
    assert_eq!(total, heard.last().unwrap().0);
    // 5,001 records of 1 KB, rounded up to whole 4 KB clusters.
    assert_eq!(5004 * 1024, total);
}

struct Build {
    _db: TempDb,
    indexer: MftVolumeIndexer,
}

fn build_over(volume: FakeVolume) -> Build {
    let db = TempDb::new();
    let access = FakeVolumeAccess::new();
    access.add("Q", volume);
    let mut indexer = MftVolumeIndexer::new(
        Arc::new(FsIndexRepository::new(db.db.clone())),
        Arc::new(DirSizeRepository::new(db.db.clone())),
        "Q",
        None,
    );
    assert!(indexer.open(&access));
    Build { _db: db, indexer }
}

#[test]
fn a_whole_build_climbs_to_99() {
    let mut build = build_over(FakeVolume::new(image()));
    let heard = Mutex::new(Vec::new());
    let report = |percent: Option<u8>| heard.lock().unwrap().push(percent.unwrap());

    build
        .indexer
        .build_initial_index(&CancelToken::none(), &report)
        .unwrap();

    let heard = heard.lock().unwrap();
    assert!(heard.windows(2).all(|pair| pair[0] < pair[1]), "{heard:?}");
    assert_eq!(99, *heard.last().unwrap(), "{heard:?}");
}

/// No `NTFS` signature, so the raw read gives up before it has a total and the USN enumeration takes
/// over. That path cannot say how far along it is, so no number is what it must show.
#[test]
fn the_names_only_fallback_reports_no_number() {
    let mut garbled = image();
    garbled[3..11].copy_from_slice(b"NOTNTFS ");
    let mut build = build_over(FakeVolume::new(garbled));
    let heard = Mutex::new(Vec::new());
    let report = |percent: Option<u8>| heard.lock().unwrap().push(percent);

    build
        .indexer
        .build_initial_index(&CancelToken::none(), &report)
        .unwrap();

    assert!(heard.lock().unwrap().is_empty());
}
