// Port of tests/BertBrowser.Core.Tests/ChangeLogRepositoryTests.cs.
use std::collections::HashSet;
use std::time::Duration;

use bb_core::FileTime;
use bb_core::changes::{ChangeEvent, ChangeKind, ChangeLogRules, ChangeQuery};
use bb_core::data::ChangeLogRepository;
use bb_paths::PathKey;
use bb_test_support::TempDb;

fn t0() -> FileTime {
    FileTime::from_ymd_hms(2026, 9, 2, 12, 0, 0)
}

fn setup() -> (TempDb, ChangeLogRepository) {
    let t = TempDb::new();
    let repo = ChangeLogRepository::new(t.db());
    (t, repo)
}

fn ev(display_path: &str, kind: ChangeKind, utc: FileTime) -> ChangeEvent {
    ChangeEvent {
        path_key: PathKey::canonicalize(display_path).unwrap(),
        display_path: display_path.to_string(),
        is_directory: false,
        hidden: false,
        kind,
        old_display_path: None,
        utc,
    }
}

fn q(since: FileTime) -> ChangeQuery {
    ChangeQuery {
        since_utc: since,
        scope_path_key: None,
        kinds: ChangeKind::all(),
        include_hidden: false,
        limit: 100,
    }
}

#[test]
fn record_coalesces_repeated_writes_inside_the_window() {
    let (_t, repo) = setup();
    repo.record(&[
        ev(r"C:\Logs\app.log", ChangeKind::Modified, t0()),
        ev(r"C:\Logs\app.log", ChangeKind::Modified, t0().add_secs(30)),
        ev(r"C:\Logs\app.log", ChangeKind::Modified, t0().add_secs(59)),
    ])
    .unwrap();

    let (rows, _) = repo.query(&q(t0().add_hours(-1))).unwrap();

    assert_eq!(1, rows.len());
    assert_eq!(3, rows[0].count);
    assert_eq!(t0(), rows[0].first_utc);
    assert_eq!(t0().add_secs(59), rows[0].last_utc);
}

#[test]
fn record_starts_a_new_row_once_the_window_has_passed() {
    let (_t, repo) = setup();
    repo.record(&[ev(r"C:\Logs\app.log", ChangeKind::Modified, t0())])
        .unwrap();
    repo.record(&[ev(
        r"C:\Logs\app.log",
        ChangeKind::Modified,
        t0().add_secs(61),
    )])
    .unwrap();

    let (rows, _) = repo.query(&q(t0().add_hours(-1))).unwrap();

    assert_eq!(2, rows.len());
    assert!(rows.iter().all(|r| r.count == 1));
}

#[test]
fn record_does_not_fold_different_kinds_of_the_same_path() {
    let (_t, repo) = setup();
    repo.record(&[
        ev(r"C:\Tmp\setup.tmp", ChangeKind::Created, t0()),
        ev(r"C:\Tmp\setup.tmp", ChangeKind::Deleted, t0().add_secs(5)),
    ])
    .unwrap();

    let (rows, _) = repo.query(&q(t0().add_hours(-1))).unwrap();
    assert_eq!(2, rows.len());
}

#[test]
fn record_keeps_the_first_rows_paths_when_folding() {
    let (_t, repo) = setup();
    let mut first = ev(r"C:\Down\installer.msi", ChangeKind::Renamed, t0());
    first.old_display_path = Some(r"C:\Down\installer.part".to_string());
    let mut second = ev(
        r"C:\Down\installer.msi",
        ChangeKind::Renamed,
        t0().add_secs(10),
    );
    second.old_display_path = Some(r"C:\Down\other.part".to_string());
    repo.record(&[first, second]).unwrap();

    let (rows, _) = repo.query(&q(t0().add_hours(-1))).unwrap();

    assert_eq!(1, rows.len());
    assert_eq!(
        Some(r"C:\Down\installer.part".to_string()),
        rows[0].old_display_path
    );
    assert_eq!(r"C:\Down\installer.msi", rows[0].display_path);
    assert_eq!(ChangeKind::Renamed, rows[0].kind);
}

#[test]
fn record_empty_batch_touches_nothing() {
    let (_t, repo) = setup();
    repo.record(&[]).unwrap();
    assert_eq!(0, repo.count().unwrap());
}

#[test]
fn query_scopes_to_the_subtree_and_not_its_neighbours() {
    let (_t, repo) = setup();
    repo.record(&[
        ev(r"C:\Foo\a.txt", ChangeKind::Created, t0()),
        ev(r"C:\Foo\Sub\b.txt", ChangeKind::Created, t0()),
        ev(r"C:\Foobar\c.txt", ChangeKind::Created, t0()),
        ev(r"D:\Foo\d.txt", ChangeKind::Created, t0()),
    ])
    .unwrap();

    let mut query = q(t0().add_hours(-1));
    query.scope_path_key = Some(PathKey::canonicalize(r"C:\Foo").unwrap());
    let (rows, _) = repo.query(&query).unwrap();

    assert_eq!(2, rows.len());
    assert!(rows.iter().all(|r| r.path_key.starts_with(r"C:\FOO\")));
}

#[test]
fn query_filters_by_kind() {
    let (_t, repo) = setup();
    repo.record(&[
        ev(r"C:\a.txt", ChangeKind::Created, t0()),
        ev(r"C:\b.txt", ChangeKind::Modified, t0()),
        ev(r"C:\c.txt", ChangeKind::Deleted, t0()),
    ])
    .unwrap();

    let mut query = q(t0().add_hours(-1));
    query.kinds = HashSet::from([ChangeKind::Created, ChangeKind::Deleted]);
    let (rows, _) = repo.query(&query).unwrap();

    assert_eq!(2, rows.len());
    assert!(!rows.iter().any(|r| r.kind == ChangeKind::Modified));
}

#[test]
fn query_hides_hidden_unless_asked() {
    let (_t, repo) = setup();
    let mut hidden = ev(r"C:\.cache\index", ChangeKind::Created, t0());
    hidden.hidden = true;
    repo.record(&[ev(r"C:\visible.txt", ChangeKind::Created, t0()), hidden])
        .unwrap();

    let (without, _) = repo.query(&q(t0().add_hours(-1))).unwrap();
    let mut with_hidden = q(t0().add_hours(-1));
    with_hidden.include_hidden = true;
    let (with, _) = repo.query(&with_hidden).unwrap();

    assert_eq!(1, without.len());
    assert_eq!(2, with.len());
}

#[test]
fn query_is_newest_first_and_bounded_below() {
    let (_t, repo) = setup();
    repo.record(&[
        ev(r"C:\old.txt", ChangeKind::Created, t0().add_hours(-5)),
        ev(r"C:\mid.txt", ChangeKind::Created, t0().add_minutes(-30)),
        ev(r"C:\new.txt", ChangeKind::Created, t0()),
    ])
    .unwrap();

    let (rows, _) = repo.query(&q(t0().add_hours(-1))).unwrap();

    let paths: Vec<&str> = rows.iter().map(|r| r.display_path.as_str()).collect();
    assert_eq!(vec![r"C:\new.txt", r"C:\mid.txt"], paths);
}

#[test]
fn query_reports_truncation_past_the_limit() {
    let (_t, repo) = setup();
    let events: Vec<ChangeEvent> = (0..5)
        .map(|i| {
            ev(
                &format!(r"C:\f{i}.txt"),
                ChangeKind::Created,
                t0().add_secs(i),
            )
        })
        .collect();
    repo.record(&events).unwrap();

    let mut query = q(t0().add_hours(-1));
    query.limit = 3;
    let (rows, truncated) = repo.query(&query).unwrap();

    assert_eq!(3, rows.len());
    assert!(truncated);
}

#[test]
fn query_needs_no_sorter() {
    // ORDER BY last_utc DESC must come off ix_fs_change_last, whatever else is filtered: a temp
    // B-tree here would mean materialising every row in range before the LIMIT could stop it.
    let (_t, repo) = setup();
    repo.record(&[ev(r"C:\Foo\a.txt", ChangeKind::Created, t0())])
        .unwrap();

    let mut scoped = q(t0().add_hours(-1));
    scoped.scope_path_key = Some(PathKey::canonicalize(r"C:\Foo").unwrap());
    let mut scoped_kind = scoped.clone();
    scoped_kind.kinds = HashSet::from([ChangeKind::Deleted]);

    for query in [q(t0().add_hours(-1)), scoped, scoped_kind] {
        let plan = repo.explain_query(&query).unwrap().join("\n");
        assert!(!plan.contains("TEMP B-TREE"), "{plan}");
        assert!(plan.contains("ix_fs_change_last"), "{plan}");
    }
}

#[test]
fn prune_drops_rows_older_than_the_retention() {
    let (_t, repo) = setup();
    repo.record(&[
        ev(r"C:\old.txt", ChangeKind::Created, t0().add_hours(-30)),
        ev(r"C:\kept.txt", ChangeKind::Created, t0().add_hours(-2)),
    ])
    .unwrap();

    repo.prune(
        t0(),
        Duration::from_secs(24 * 3600),
        ChangeLogRules::MAX_ROWS,
    )
    .unwrap();

    let (rows, _) = repo.query(&q(t0().add_days(-7))).unwrap();
    assert_eq!(1, rows.len());
    assert_eq!(r"C:\kept.txt", rows[0].display_path);
}

#[test]
fn prune_caps_the_table_keeping_the_newest() {
    let (_t, repo) = setup();
    let events: Vec<ChangeEvent> = (0..10)
        .map(|i| {
            ev(
                &format!(r"C:\f{i}.txt"),
                ChangeKind::Created,
                t0().add_secs(i),
            )
        })
        .collect();
    repo.record(&events).unwrap();

    repo.prune(t0().add_minutes(1), Duration::from_secs(24 * 3600), 4)
        .unwrap();

    let (rows, _) = repo.query(&q(t0().add_hours(-1))).unwrap();
    let paths: Vec<&str> = rows.iter().map(|r| r.display_path.as_str()).collect();
    assert_eq!(
        vec![r"C:\f9.txt", r"C:\f8.txt", r"C:\f7.txt", r"C:\f6.txt"],
        paths
    );
}

#[test]
fn prune_under_the_cap_is_a_no_op() {
    let (_t, repo) = setup();
    repo.record(&[ev(r"C:\a.txt", ChangeKind::Created, t0())])
        .unwrap();

    repo.prune(t0(), Duration::from_secs(24 * 3600), 4).unwrap();

    assert_eq!(1, repo.count().unwrap());
}

#[test]
fn clear_empties_the_table() {
    let (_t, repo) = setup();
    repo.record(&[
        ev(r"C:\a.txt", ChangeKind::Created, t0()),
        ev(r"C:\b.txt", ChangeKind::Created, t0()),
    ])
    .unwrap();

    repo.clear().unwrap();

    assert_eq!(0, repo.count().unwrap());
}

#[test]
fn stamp_moves_when_a_row_is_folded_not_only_when_one_is_added() {
    let (_t, repo) = setup();
    let empty = repo.stamp().unwrap();
    repo.record(&[ev(r"C:\a.txt", ChangeKind::Modified, t0())])
        .unwrap();
    let first = repo.stamp().unwrap();
    repo.record(&[ev(r"C:\a.txt", ChangeKind::Modified, t0().add_secs(10))])
        .unwrap();
    let folded = repo.stamp().unwrap();

    assert_ne!(empty, first);
    assert_ne!(first, folded);
    assert_eq!(1, repo.count().unwrap());
}

#[test]
fn query_reads_back_every_column() {
    let (_t, repo) = setup();
    let mut dir = ev(r"C:\Proj\bin", ChangeKind::Created, t0());
    dir.is_directory = true;
    dir.hidden = true;
    repo.record(&[dir]).unwrap();

    let mut query = q(t0().add_hours(-1));
    query.include_hidden = true;
    let (rows, _) = repo.query(&query).unwrap();

    assert_eq!(1, rows.len());
    let row = &rows[0];
    assert_eq!(r"C:\PROJ\BIN", row.path_key);
    assert_eq!(r"C:\Proj\bin", row.display_path);
    assert!(row.is_directory);
    assert!(row.hidden);
    assert_eq!(None, row.old_display_path);
    assert_eq!(t0(), row.last_utc);
}
