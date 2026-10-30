// Port of tests/BertBrowser.Core.Tests/FsIndexRepositoryTests.cs. Index rows are synthetic: no real
// filesystem is needed.
use bb_core::FileTime;
use bb_core::attributes::{ARCHIVE, READONLY};
use bb_core::cancel::CancelToken;
use bb_core::data::{FsEntryRow, FsIndexRepository};
use bb_core::search::SearchQuery;
use bb_paths::PathKey;
use bb_test_support::TempDb;

fn setup() -> (TempDb, FsIndexRepository) {
    let t = TempDb::new();
    let repo = FsIndexRepository::new(t.db());
    (t, repo)
}

struct Row(FsEntryRow);

fn row(display_path: &str) -> Row {
    Row(FsEntryRow {
        path_key: PathKey::canonicalize(display_path).unwrap(),
        name: bb_paths::file_name(display_path).to_string(),
        is_directory: false,
        size_bytes: 0,
        modified_utc: FileTime::now(),
        hidden: false,
        attributes: 0,
        created_utc: FileTime::UNKNOWN,
    })
}

impl Row {
    fn dir(mut self) -> Row {
        self.0.is_directory = true;
        self
    }
    fn size(mut self, size: i64) -> Row {
        self.0.size_bytes = size;
        self
    }
    fn hidden(mut self) -> Row {
        self.0.hidden = true;
        self
    }
    fn attributes(mut self, a: u32) -> Row {
        self.0.attributes = a;
        self
    }
    fn created(mut self, t: FileTime) -> Row {
        self.0.created_utc = t;
        self
    }
}

fn upsert(repo: &FsIndexRepository, rows: Vec<Row>, crawl_gen: i64) {
    let rows: Vec<FsEntryRow> = rows.into_iter().map(|r| r.0).collect();
    repo.upsert_entries(&rows, crawl_gen).unwrap();
}

fn q(text: &str) -> SearchQuery {
    SearchQuery::parse(text).query.unwrap()
}

fn names(repo: &FsIndexRepository, root: &str, query: &str) -> Vec<String> {
    let (hits, _) = repo.search(root, &q(query), 100, true).unwrap();
    let mut names: Vec<String> = hits.into_iter().map(|h| h.name).collect();
    names.sort();
    names
}

// --- attributes and created round-trip ---

#[test]
fn search_carries_attributes_and_created_back_onto_the_hit() {
    let (_t, repo) = setup();
    let created = FileTime::from_ymd_hms(2026, 3, 4, 5, 6, 7);
    upsert(
        &repo,
        vec![
            row(r"C:\Data\report.txt")
                .attributes(READONLY | ARCHIVE)
                .created(created),
        ],
        1,
    );

    let (hits, _) = repo.search(r"C:\Data", &q("report"), 100, true).unwrap();
    assert_eq!(1, hits.len());
    assert_eq!(READONLY | ARCHIVE, hits[0].attributes);
    assert_eq!(created, hits[0].created_utc);
}

/// A row whose creation time was never recorded stores unknown, which reads back as unknown and
/// satisfies no dc: filter rather than every open-ended one.
#[test]
fn search_reads_an_unrecorded_created_date_without_throwing() {
    let (t, repo) = setup();
    upsert(&repo, vec![row(r"C:\Data\report.txt")], 1);
    t.db.open()
        .unwrap()
        .execute("UPDATE fs_entry SET created_utc = 0;", [])
        .unwrap();

    let (hits, _) = repo.search(r"C:\Data", &q("report"), 100, true).unwrap();
    assert_eq!(1, hits.len());
    assert_eq!(FileTime::UNKNOWN, hits[0].created_utc);

    assert!(
        repo.search(r"C:\Data", &q("report dc:>2000"), 100, true)
            .unwrap()
            .0
            .is_empty()
    );
    assert!(
        repo.search(r"C:\Data", &q("report dc:<2100"), 100, true)
            .unwrap()
            .0
            .is_empty()
    );
}

/// The attribute filter runs in SQL as a mask test.
#[test]
fn search_filters_on_one_attribute_bit_of_a_mask() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\locked.txt").attributes(READONLY | ARCHIVE),
            row(r"C:\Data\plain.txt").attributes(ARCHIVE),
            row(r"C:\Data\legacy.txt"), // never recorded: 0 must not match anything
        ],
        1,
    );

    assert_eq!(
        vec!["locked.txt"],
        names(&repo, r"C:\Data", "txt is:readonly")
    );
    assert_eq!(2, names(&repo, r"C:\Data", "txt is:archived").len());
    assert!(names(&repo, r"C:\Data", "txt is:system").is_empty());
}

#[test]
fn search_matches_substring_case_insensitively() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\Quarterly-Report.docx").size(42),
            row(r"C:\Data\notes.txt"),
        ],
        1,
    );

    let (hits, truncated) = repo.search(r"C:\data", &q("report"), 100, true).unwrap();

    assert_eq!(1, hits.len());
    assert_eq!("Quarterly-Report.docx", hits[0].name);
    assert_eq!(42, hits[0].size_bytes);
    assert!(!hits[0].is_directory);
    assert!(!truncated);
}

#[test]
fn search_excludes_hidden_unless_requested() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\report-visible.txt"),
            row(r"C:\Data\report-secret.txt").hidden(),
        ],
        1,
    );

    let (all, _) = repo.search(r"C:\Data", &q("report"), 100, true).unwrap();
    assert_eq!(2, all.len());
    assert!(
        all.iter()
            .find(|h| h.name == "report-secret.txt")
            .unwrap()
            .hidden
    );

    let (visible, _) = repo.search(r"C:\Data", &q("report"), 100, false).unwrap();
    assert_eq!(1, visible.len());
    assert_eq!("report-visible.txt", visible[0].name);
    assert!(!visible[0].hidden);
}

#[test]
fn search_global_spans_drives_with_full_paths() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data").dir(),
            row(r"C:\Data\report.txt").size(5),
            row(r"C:\readme-report.md"), // direct child of the drive root
            row(r"D:\Backup").dir(),
            row(r"D:\Backup\report-old.txt"),
            row(r"C:\Data\unrelated.txt"),
        ],
        1,
    );

    let (hits, truncated) = repo.search_global(&q("report"), 100, true).unwrap();

    assert!(!truncated);
    let mut paths: Vec<&str> = hits.iter().map(|h| h.display_path.as_str()).collect();
    paths.sort();
    assert_eq!(
        vec![
            r"C:\Data\report.txt",
            r"C:\readme-report.md",
            r"D:\Backup\report-old.txt"
        ],
        paths
    );

    // The Folder column carries the full parent path, not a relative one.
    assert_eq!(
        r"C:\Data",
        hits.iter()
            .find(|h| h.name == "report.txt")
            .unwrap()
            .relative_dir_display
    );
    assert_eq!(
        r"C:\",
        hits.iter()
            .find(|h| h.name == "readme-report.md")
            .unwrap()
            .relative_dir_display
    );
}

#[test]
fn search_global_honors_hidden_and_truncation() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\a-report.txt"),
            row(r"C:\b-report.txt"),
            row(r"C:\c-report.txt").hidden(),
        ],
        1,
    );

    let (visible, _) = repo.search_global(&q("report"), 100, false).unwrap();
    assert_eq!(2, visible.len());
    assert!(!visible.iter().any(|h| h.name == "c-report.txt"));

    let (capped, truncated) = repo.search_global(&q("report"), 1, true).unwrap();
    assert!(truncated);
    assert_eq!(1, capped.len());
}

#[test]
fn search_scopes_to_subtree_sibling_prefix_excluded() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![row(r"C:\Foo\match.txt"), row(r"C:\Foobar\match.txt")],
        1,
    );

    let (hits, _) = repo.search(r"C:\Foo", &q("match"), 100, true).unwrap();
    assert_eq!(1, hits.len());
    assert_eq!(r"C:\Foo\match.txt", hits[0].display_path);
}

#[test]
fn search_and_semantics_all_terms_must_match() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\Project-Report.docx"),
            row(r"C:\Data\Project-Plan.docx"),
            row(r"C:\Data\Old-Report.docx"),
        ],
        1,
    );

    assert_eq!(
        vec!["Project-Report.docx"],
        names(&repo, r"C:\Data", "proj rep")
    );
}

#[test]
fn search_wildcards_and_brackets() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\b.txt"),
            row(r"C:\Data\sub\deep.txt"),
            row(r"C:\Data\sub").dir(),
            row(r"C:\Data\file.bin"),
            row(r"C:\Data\a[1].tmp"),
        ],
        1,
    );

    for (query, expected) in [
        ("*.txt", vec!["b.txt", "deep.txt"]),
        ("f?le", vec!["file.bin"]),
        ("a[1]", vec!["a[1].tmp"]),
    ] {
        assert_eq!(expected, names(&repo, r"C:\Data", query), "{query}");
    }
}

#[test]
fn search_caps_and_reports_truncation() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        (0..5)
            .map(|i| row(&format!(r"C:\Data\file{i}.txt")))
            .collect(),
        1,
    );

    let (hits, truncated) = repo.search(r"C:\Data", &q("file"), 3, true).unwrap();
    assert_eq!(3, hits.len());
    assert!(truncated);
}

#[test]
fn search_reconstructs_relative_display_paths() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Root\Sub").dir(),
            row(r"C:\Root\Sub\Deep").dir(),
            row(r"C:\Root\Sub\Deep\File.txt"),
            row(r"C:\Root\direct.txt"),
        ],
        1,
    );

    let (hits, _) = repo.search(r"C:\Root", &q("file"), 100, true).unwrap();
    let nested = hits.iter().find(|h| h.name == "File.txt").unwrap();
    assert_eq!(r"Sub\Deep", nested.relative_dir_display);
    assert_eq!(r"C:\Root\Sub\Deep\File.txt", nested.display_path);

    let (direct, _) = repo.search(r"C:\Root", &q("direct"), 100, true).unwrap();
    assert_eq!("", direct[0].relative_dir_display);
    assert_eq!(r"C:\Root\direct.txt", direct[0].display_path);
}

#[test]
fn sweep_vanished_removes_only_untouched_rows_in_range() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\stays.txt"),
            row(r"C:\Data\vanished.txt"),
            row(r"C:\Other\outside.txt"),
        ],
        1,
    );

    // A newer crawl re-touched only stays.txt.
    upsert(&repo, vec![row(r"C:\Data\stays.txt")], 2);
    repo.sweep_vanished(&PathKey::canonicalize(r"C:\Data").unwrap(), 2)
        .unwrap();

    assert_eq!(vec!["stays.txt"], names(&repo, r"C:\Data", "txt"));
    assert_eq!(1, names(&repo, r"C:\Other", "txt").len()); // outside the swept range
}

#[test]
fn find_covering_root_ancestor_or_equal_complete_required() {
    let (_t, repo) = setup();
    assert!(repo.find_covering_root(r"C:\Foo\Bar").unwrap().is_none());

    let foo = PathKey::canonicalize(r"C:\Foo").unwrap();
    repo.upsert_root(&foo, r"C:\Foo", FileTime::now(), false)
        .unwrap();
    assert!(repo.find_covering_root(r"C:\Foo\Bar").unwrap().is_none()); // incomplete crawl does not cover

    repo.upsert_root(&foo, r"C:\Foo", FileTime::now(), true)
        .unwrap();
    assert_eq!(
        foo,
        repo.find_covering_root(r"C:\Foo\Bar\baz")
            .unwrap()
            .unwrap()
            .path_key
    );
    assert_eq!(
        foo,
        repo.find_covering_root(r"C:\Foo")
            .unwrap()
            .unwrap()
            .path_key
    ); // equal counts

    assert!(repo.find_covering_root(r"C:\Other").unwrap().is_none());
    assert!(repo.find_covering_root(r"C:\Foobar").unwrap().is_none()); // sibling prefix is not covered
}

#[test]
fn find_covering_root_prefers_non_stale() {
    let (_t, repo) = setup();
    let bar = PathKey::canonicalize(r"C:\Foo\Bar").unwrap();
    let foo = PathKey::canonicalize(r"C:\Foo").unwrap();
    repo.upsert_root(&bar, r"C:\Foo\Bar", FileTime::now(), true)
        .unwrap();
    repo.mark_root_stale(&bar).unwrap();
    repo.upsert_root(&foo, r"C:\Foo", FileTime::now(), true)
        .unwrap();

    // The deeper root is stale; the fresh ancestor wins.
    let covering = repo.find_covering_root(r"C:\Foo\Bar\baz").unwrap().unwrap();
    assert_eq!(foo, covering.path_key);
    assert!(!covering.stale);
}

#[test]
fn delete_subtree_removes_entry_and_descendants_only() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\Sub").dir(),
            row(r"C:\Data\Sub\a.txt"),
            row(r"C:\Data\Sub\deep\b.txt"),
            row(r"C:\Data\Subtle.txt"), // shares the name prefix; must survive
        ],
        1,
    );

    repo.delete_subtree(&PathKey::canonicalize(r"C:\Data\Sub").unwrap())
        .unwrap();

    assert_eq!(vec!["Subtle.txt"], names(&repo, r"C:\Data", "txt sub"));
}

#[test]
fn rename_rewrites_descendant_keys_in_place() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\A\Old").dir(),
            row(r"C:\A\Old\f1.txt"),
            row(r"C:\A\Old\sub").dir(),
            row(r"C:\A\Old\sub\f2.txt"),
            row(r"C:\A\other.txt"),
        ],
        1,
    );

    repo.rename(
        &PathKey::canonicalize(r"C:\A\Old").unwrap(),
        &PathKey::canonicalize(r"C:\A\Renamed").unwrap(),
        "Renamed",
        2,
    )
    .unwrap();

    let (hits, _) = repo.search(r"C:\A", &q("f2"), 100, true).unwrap();
    assert_eq!(1, hits.len());
    assert_eq!(r"Renamed\sub", hits[0].relative_dir_display);
    assert_eq!(r"C:\A\Renamed\sub\f2.txt", hits[0].display_path);

    assert!(names(&repo, r"C:\A\Old", "f1").is_empty()); // old subtree gone
    assert_eq!(1, names(&repo, r"C:\A\Renamed", "f1").len()); // new subtree intact
    assert_eq!(1, names(&repo, r"C:\A", "renamed").len()); // dir row renamed too
}

#[test]
fn rename_case_only_updates_display_name_only() {
    let (_t, repo) = setup();
    upsert(&repo, vec![row(r"C:\A\readme.txt")], 1);

    let key = PathKey::canonicalize(r"C:\A\readme.txt").unwrap();
    repo.rename(&key, &key, "README.txt", 2).unwrap();

    assert_eq!(vec!["README.txt"], names(&repo, r"C:\A", "readme"));
}

#[test]
fn rename_overwrites_existing_target() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\A\Src").dir(),
            row(r"C:\A\Src\new.txt"),
            row(r"C:\A\Dst").dir(),
            row(r"C:\A\Dst\stale.txt"),
        ],
        1,
    );

    repo.rename(
        &PathKey::canonicalize(r"C:\A\Src").unwrap(),
        &PathKey::canonicalize(r"C:\A\Dst").unwrap(),
        "Dst",
        2,
    )
    .unwrap();

    assert!(names(&repo, r"C:\A\Dst", "stale").is_empty());
    assert_eq!(1, names(&repo, r"C:\A\Dst", "new").len());
}

// --- largest_files (disk-space explorer) ---

#[test]
fn largest_files_orders_by_size_descending_and_respects_limit() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\small.bin").size(10),
            row(r"C:\Data\huge.bin").size(3_000),
            row(r"C:\Data\medium.bin").size(200),
        ],
        1,
    );

    let files = repo.largest_files(Some(r"C:\Data"), 2, true).unwrap();
    let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(vec!["huge.bin", "medium.bin"], names);
    assert_eq!(3_000, files[0].size_bytes);
}

/// The half-open-bounds guarantee: a scope of "C:\Data" reaches neither neighbour whose name merely
/// starts with it. `C:\DATAX` is turned away by the lower bound alone; `C:\DATA_OLD` only by the
/// upper bound, since `_` sorts above `]`.
#[test]
fn largest_files_scoped_to_the_subtree() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\big.bin").size(10_000_000_000),
            row(r"C:\Datax\bigger.bin").size(20_000_000_000),
            row(r"C:\Data_old\biggest.bin").size(30_000_000_000),
        ],
        1,
    );

    let files = repo.largest_files(Some(r"C:\Data"), 10, true).unwrap();
    assert_eq!(1, files.len());
    assert_eq!("big.bin", files[0].name);
}

#[test]
fn largest_files_excludes_directories() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\Sub").dir().size(9_000_000),
            row(r"C:\Data\file.bin").size(5),
        ],
        1,
    );

    let files = repo.largest_files(Some(r"C:\Data"), 10, true).unwrap();
    assert_eq!(1, files.len());
    assert_eq!("file.bin", files[0].name);
}

#[test]
fn largest_files_excludes_hidden_unless_requested() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\visible.bin").size(10),
            row(r"C:\Data\secret.bin").size(5_000).hidden(),
        ],
        1,
    );

    assert_eq!(
        2,
        repo.largest_files(Some(r"C:\Data"), 10, true)
            .unwrap()
            .len()
    );

    let visible_only = repo.largest_files(Some(r"C:\Data"), 10, false).unwrap();
    assert_eq!(1, visible_only.len());
    assert_eq!("visible.bin", visible_only[0].name);
}

#[test]
fn largest_files_reconstructs_display_paths() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\Sub").dir(),
            row(r"C:\Data\Sub\deep.bin").size(77),
        ],
        1,
    );

    let files = repo.largest_files(Some(r"C:\Data"), 10, true).unwrap();
    assert_eq!(1, files.len());
    assert_eq!("Sub", files[0].relative_dir_display);
    assert_eq!(r"C:\Data\Sub\deep.bin", files[0].display_path);
}

#[test]
fn largest_files_whole_pc_spans_volumes_and_carries_full_paths() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data").dir(),
            row(r"C:\Data\c-file.bin").size(100),
            row(r"D:\Media").dir(),
            row(r"D:\Media\d-file.bin").size(900),
        ],
        1,
    );

    let files = repo.largest_files(None, 10, true).unwrap();
    let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(vec!["d-file.bin", "c-file.bin"], names);
    assert_eq!(r"D:\Media\d-file.bin", files[0].display_path);
    assert_eq!(r"D:\Media", files[0].relative_dir_display);
}

#[test]
fn largest_files_non_positive_limit_returns_nothing_rather_than_everything() {
    let (_t, repo) = setup();
    upsert(&repo, vec![row(r"C:\Data\file.bin").size(5)], 1);
    assert!(
        repo.largest_files(Some(r"C:\Data"), 0, true)
            .unwrap()
            .is_empty()
    );
}

// --- duplicate_candidates: the shortlist a duplicate scan starts from ---

fn candidates(
    repo: &FsIndexRepository,
    root: Option<&str>,
    min: i64,
    hidden: bool,
) -> bb_core::data::DuplicateShortlist {
    repo.duplicate_candidates(root, min, hidden, None, &CancelToken::none())
        .unwrap()
}

#[test]
fn duplicate_candidates_returns_only_colliding_lengths() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\a.bin").size(4096),
            row(r"C:\Data\b.bin").size(4096),
            row(r"C:\Data\lonely.bin").size(999),
        ],
        1,
    );

    let shortlist = candidates(&repo, Some(r"C:\Data"), 1, true);
    assert_eq!(2, shortlist.files.len());
    assert!(shortlist.files.iter().all(|f| f.size_bytes == 4096));
}

#[test]
fn duplicate_candidates_never_returns_directories() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\One").dir(),
            row(r"C:\Data\Two").dir(),
            row(r"C:\Data\a.bin").size(4096),
            row(r"C:\Data\b.bin").size(4096),
        ],
        1,
    );

    let shortlist = candidates(&repo, Some(r"C:\Data"), 1, true);
    assert_eq!(2, shortlist.files.len());
    assert!(shortlist.files.iter().all(|f| !f.is_directory));
}

#[test]
fn duplicate_candidates_honours_the_size_floor() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\small-a.bin").size(10),
            row(r"C:\Data\small-b.bin").size(10),
            row(r"C:\Data\big-a.bin").size(5000),
            row(r"C:\Data\big-b.bin").size(5000),
        ],
        1,
    );

    let shortlist = candidates(&repo, Some(r"C:\Data"), 1000, true);
    assert_eq!(2, shortlist.files.len());
    assert!(shortlist.files.iter().all(|f| f.size_bytes == 5000));
}

#[test]
fn duplicate_candidates_zero_length_files_are_never_candidates() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\empty-a.bin").size(0),
            row(r"C:\Data\empty-b.bin").size(0),
        ],
        1,
    );
    assert!(
        candidates(&repo, Some(r"C:\Data"), 0, true)
            .files
            .is_empty()
    );
}

#[test]
fn duplicate_candidates_excludes_hidden_unless_requested() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\visible.bin").size(4096),
            row(r"C:\Data\secret.bin").size(4096).hidden(),
        ],
        1,
    );

    assert_eq!(2, candidates(&repo, Some(r"C:\Data"), 1, true).files.len());
    // With the hidden row gone the other has nothing to collide with, so the pair vanishes.
    assert!(
        candidates(&repo, Some(r"C:\Data"), 1, false)
            .files
            .is_empty()
    );
}

/// The exclusion runs in both passes. Applying it only to the second would let an excluded file prop
/// up a size group that then comes back with a single member.
#[test]
fn duplicate_candidates_an_excluded_file_cannot_prop_up_a_group() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\keep.bin").size(4096),
            row(r"C:\Data\skip.bin").size(4096),
        ],
        1,
    );

    let exclude = |key: &str| key.ends_with(r"\SKIP.BIN");
    let shortlist = repo
        .duplicate_candidates(
            Some(r"C:\Data"),
            1,
            true,
            Some(&exclude),
            &CancelToken::none(),
        )
        .unwrap();
    assert!(shortlist.files.is_empty());
}

#[test]
fn duplicate_candidates_scopes_to_the_subtree() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\a.bin").size(4096),
            row(r"C:\Data\b.bin").size(4096),
            row(r"C:\Other\a.bin").size(4096),
        ],
        1,
    );

    assert_eq!(2, candidates(&repo, Some(r"C:\Data"), 1, true).files.len());
    assert_eq!(3, candidates(&repo, None, 1, true).files.len());
}

#[test]
fn duplicate_candidates_rebuilds_display_paths_from_ancestors() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\Nested").dir(),
            row(r"C:\Data\Nested\Report.pdf").size(4096),
            row(r"C:\Data\Report.pdf").size(4096),
        ],
        1,
    );

    let shortlist = candidates(&repo, Some(r"C:\Data"), 1, true);
    let nested = shortlist
        .files
        .iter()
        .find(|f| !f.relative_dir_display.is_empty())
        .unwrap();
    assert_eq!("Nested", nested.relative_dir_display);
    assert_eq!(r"C:\Data\Nested\Report.pdf", nested.display_path);
}

/// The two scope counts describe what the index holds rather than what this caller asked to see,
/// so they are taken before both the floor and the exclusion.
#[test]
fn duplicate_candidates_counts_what_the_index_holds_not_what_was_asked_for() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![
            row(r"C:\Data\tiny.bin").size(1),
            row(r"C:\Data\a.bin").size(4096),
            row(r"C:\Data\b.bin").size(4096),
        ],
        1,
    );

    let exclude = |_: &str| true;
    let shortlist = repo
        .duplicate_candidates(
            Some(r"C:\Data"),
            4096,
            true,
            Some(&exclude),
            &CancelToken::none(),
        )
        .unwrap();
    assert!(shortlist.files.is_empty());
    assert_eq!(3, shortlist.files_in_scope);
    assert_eq!(3, shortlist.sized_files_in_scope);
}

/// Rows in scope, not one with a length: the names-only build, which has to be distinguishable from
/// an empty folder.
#[test]
fn duplicate_candidates_a_sizeless_index_is_rows_with_no_lengths() {
    let (_t, repo) = setup();
    upsert(
        &repo,
        vec![row(r"C:\Data\a.bin").size(0), row(r"C:\Data\b.bin").size(0)],
        1,
    );

    let sizeless = candidates(&repo, Some(r"C:\Data"), 1, true);
    assert_eq!(2, sizeless.files_in_scope);
    assert_eq!(0, sizeless.sized_files_in_scope);

    let empty = candidates(&repo, Some(r"C:\Nowhere"), 1, true);
    assert_eq!(0, empty.files_in_scope);
    assert_eq!(0, empty.sized_files_in_scope);
}
