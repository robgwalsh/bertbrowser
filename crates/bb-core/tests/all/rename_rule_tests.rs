// Port of tests/BertBrowser.Core.Tests/RenameRuleTests.cs. The advanced naming rule: find/replace,
// a case transform, a counter and a date, placed by a template. Pure — no disk, and no clock,
// because the date rides on the source.
use bb_core::FileTime;
use bb_core::rename::{RenameCase, RenamePattern, RenameRule, RenameScope, RenameSource};

/// 2026-03-09 17:04:22, as the local wall-clock time a source carries.
fn stamped() -> FileTime {
    FileTime::from_ymd_hms(2026, 3, 9, 17, 4, 22)
}

fn file(path: &str) -> RenameSource {
    RenameSource::file(path)
}

fn file_at(path: &str, modified: FileTime) -> RenameSource {
    RenameSource::new(path, false, Some(modified))
}

fn dir(path: &str) -> RenameSource {
    RenameSource::dir(path)
}

fn rule(template: &str) -> RenameRule {
    RenameRule::new(template)
}

/// The names a rule gives, with each item's own problem discarded.
fn names(sources: &[RenameSource], rule: &RenameRule) -> Vec<String> {
    RenamePattern::apply(sources, rule)
        .into_iter()
        .map(|n| n.name)
        .collect()
}

fn name(source: RenameSource, rule: &RenameRule) -> String {
    names(&[source], rule).remove(0)
}

// --- the template ---

#[test]
fn name_is_the_whole_file_name_as_command_template_already_spells_it() {
    assert_eq!(
        "report.pdf",
        name(file(r"C:\p\report.pdf"), &rule("{name}"))
    );
}

#[test]
fn base_is_the_name_without_its_extension() {
    assert_eq!("report", name(file(r"C:\p\report.pdf"), &rule("{base}")));
}

#[test]
fn ext_carries_its_own_dot() {
    assert_eq!("x.pdf", name(file(r"C:\p\report.pdf"), &rule("x{ext}")));
}

#[test]
fn base_dot_ext_doubles_the_dot_because_ext_already_has_one() {
    // Legal, and Validate accepts it — the dot is in the middle, where Clean cannot reach.
    // Pinned so the token hint keeps telling people {ext} brings its own dot.
    assert_eq!(
        "report..pdf",
        name(file(r"C:\p\report.pdf"), &rule("{base}.{ext}"))
    );
}

#[test]
fn base_dot_ext_on_an_extensionless_file_loses_the_stray_dot() {
    assert_eq!(
        "LICENSE",
        name(file(r"C:\p\LICENSE"), &rule("{base}.{ext}"))
    );
}

#[test]
fn parent_is_the_folders_name_never_its_path() {
    assert_eq!(
        "Photos-a.jpg",
        name(file(r"C:\Users\Rob\Photos\a.jpg"), &rule("{parent}-{name}"))
    );
}

#[test]
fn tokens_are_case_insensitive() {
    assert_eq!(
        "report.pdf",
        name(file(r"C:\p\report.pdf"), &rule("{NAME}"))
    );
}

#[test]
fn doubled_braces_are_literal_braces() {
    assert_eq!(
        "{report}",
        name(file(r"C:\p\report.pdf"), &rule("{{{base}}}"))
    );
}

#[test]
fn a_folder_has_no_extension_to_place() {
    assert_eq!("Work", name(dir(r"C:\p\My.Project"), &rule("Work{ext}")));
}

// --- find and replace ---

#[test]
fn replace_rewrites_the_stem_and_leaves_the_extension() {
    let rule = RenameRule {
        find: "IMG_".into(),
        replace: "Holiday".into(),
        ..rule("{name}")
    };
    assert_eq!("Holiday0001.jpg", name(file(r"C:\p\IMG_0001.jpg"), &rule));
}

#[test]
fn replace_ignores_case_unless_asked() {
    let rule = RenameRule {
        find: "img_".into(),
        replace: "Holiday".into(),
        ..rule("{name}")
    };
    assert_eq!("Holiday0001.jpg", name(file(r"C:\p\IMG_0001.jpg"), &rule));
}

#[test]
fn replace_with_match_case_leaves_the_other_casing_alone() {
    let rule = RenameRule {
        find: "img_".into(),
        replace: "Holiday".into(),
        match_case: true,
        ..rule("{name}")
    };
    assert_eq!("IMG_0001.jpg", name(file(r"C:\p\IMG_0001.jpg"), &rule));
}

#[test]
fn replace_trims_the_space_it_leaves_behind() {
    // "report v2.txt" losing "v2" leaves "report " — and the final clean cannot see it,
    // because by then the name ends in ".txt".
    let rule = RenameRule {
        find: "v2".into(),
        replace: String::new(),
        ..rule("{name}")
    };
    assert_eq!("report.txt", name(file(r"C:\p\report v2.txt"), &rule));
}

#[test]
fn replace_scoped_to_the_extension_leaves_the_stem_alone() {
    let rule = RenameRule {
        find: "jpeg".into(),
        replace: "jpg".into(),
        scope: RenameScope::Extension,
        ..rule("{name}")
    };
    assert_eq!(
        "jpeg-holiday.jpg",
        name(file(r"C:\p\jpeg-holiday.jpeg"), &rule)
    );
}

#[test]
fn replace_scoped_to_the_whole_name_reaches_the_extension_too() {
    let rule = RenameRule {
        find: "a".into(),
        replace: "b".into(),
        scope: RenameScope::WholeName,
        ..rule("{name}")
    };
    assert_eq!("bbc.tbr", name(file(r"C:\p\aac.tar"), &rule));
}

#[test]
fn regex_substitutes_its_own_groups() {
    // The reason no {\1} token exists: $1 already reorders captures.
    let rule = RenameRule {
        find: r"^(.*)_(.*)$".into(),
        replace: "$2_$1".into(),
        use_regex: true,
        ..rule("{name}")
    };
    assert_eq!(
        "2026_report.txt",
        name(file(r"C:\p\report_2026.txt"), &rule)
    );
}

#[test]
fn regex_that_cannot_compile_is_reported_rather_than_thrown() {
    let rule = RenameRule {
        find: "(unclosed".into(),
        replace: "x".into(),
        use_regex: true,
        ..rule("{name}")
    };
    assert!(RenamePattern::validate_rule(&rule).is_some());
    // And Apply itself still answers, because the planner calls it unguarded.
    assert_eq!(1, RenamePattern::apply(&[file(r"C:\p\a.txt")], &rule).len());
}

#[test]
fn regex_that_runs_away_gives_up_on_that_item_instead_of_hanging_the_dialog() {
    // Catastrophic backtracking, three keystrokes from any Find box. The C# engine needed a
    // deadline to turn this into a message; this engine runs a pattern with no fancy features in
    // linear time (docs/regex-dialect.md), so what is pinned is that the dialog answers at
    // once rather than that it gives up — and a pattern that does hit the backtrack limit is
    // reported per item the same way the deadline was.
    let rule = RenameRule {
        find: "(a+)+$".into(),
        replace: "x".into(),
        use_regex: true,
        scope: RenameScope::WholeName,
        ..rule("{name}")
    };
    let runaway = file(&format!(r"C:\p\{}b.txt", "a".repeat(40)));

    let started = std::time::Instant::now();
    let result = RenamePattern::apply(&[runaway], &rule);

    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert_eq!(1, result.len());
    assert!(result[0].problem.is_some() || result[0].name == format!("{}b.txt", "a".repeat(40)));
}

// --- case ---

#[test]
fn case_rewrites_the_stem() {
    for (kind, expected) in [
        (RenameCase::Lower, "holiday photo"),
        (RenameCase::Upper, "HOLIDAY PHOTO"),
        (RenameCase::Title, "Holiday Photo"),
        (RenameCase::Sentence, "Holiday photo"),
    ] {
        let rule = RenameRule {
            case: kind,
            ..rule("{base}")
        };
        assert_eq!(
            expected,
            name(file(r"C:\p\HOLIDAY PHOTO.jpg"), &rule),
            "{kind:?}"
        );
    }
}

#[test]
fn title_case_works_on_all_caps_which_is_why_anyone_reaches_for_it() {
    // ToTitleCase leaves an already-upper-case word exactly as it found it, so this needs the
    // lower-casing pass first or the button looks dead.
    let rule = RenameRule {
        case: RenameCase::Title,
        ..rule("{base}")
    };
    assert_eq!(
        "Annual Report",
        name(file(r"C:\p\ANNUAL REPORT.docx"), &rule)
    );
}

#[test]
fn case_is_invariant_so_a_turkish_machine_does_not_mangle_the_batch() {
    // Under tr-TR, "FILE".ToLower() is "fıle" with a dotless i — a different name, on every
    // item, on somebody else's computer. There is no culture to set here: the mapping is the
    // simple per-character one `PathKey` keeps, for the same reason.
    let rule = RenameRule {
        case: RenameCase::Lower,
        ..rule("{base}")
    };
    assert_eq!("file", name(file(r"C:\p\FILE.txt"), &rule));
}

#[test]
fn case_leaves_the_extension_alone_unless_scoped_to_it() {
    let rule = RenameRule {
        case: RenameCase::Lower,
        ..rule("{name}")
    };
    assert_eq!("report.TXT", name(file(r"C:\p\REPORT.TXT"), &rule));
}

// --- the counter ---

#[test]
fn counter_numbers_in_the_order_given() {
    let rule = rule("Photo {n}{ext}");
    assert_eq!(
        vec!["Photo 1.jpg", "Photo 2.png"],
        names(&[file(r"C:\p\a.jpg"), file(r"C:\p\b.png")], &rule)
    );
}

#[test]
fn counter_honours_start_and_step() {
    let rule = RenameRule {
        counter_start: 10,
        counter_step: 5,
        ..rule("Photo {n}{ext}")
    };
    assert_eq!(
        vec!["Photo 10.jpg", "Photo 15.jpg"],
        names(&[file(r"C:\p\a.jpg"), file(r"C:\p\b.jpg")], &rule)
    );
}

#[test]
fn counter_pads_to_the_width_written_in_the_token() {
    let rule = rule("Photo {n:000}{ext}");
    assert_eq!(
        vec!["Photo 001.jpg", "Photo 002.jpg"],
        names(&[file(r"C:\p\a.jpg"), file(r"C:\p\b.jpg")], &rule)
    );
}

#[test]
fn counter_counting_down_keeps_its_sign_outside_the_padding() {
    let rule = RenameRule {
        counter_start: 1,
        counter_step: -2,
        ..rule("n{n:000}")
    };
    assert_eq!(
        vec!["n001", "n-001"],
        names(&[file(r"C:\p\a"), file(r"C:\p\b")], &rule)
    );
}

#[test]
fn counter_with_a_step_of_zero_is_refused_rather_than_colliding_later() {
    let rule = RenameRule {
        counter_step: 0,
        ..rule("{n}")
    };
    assert!(RenamePattern::validate_rule(&rule).is_some());
}

#[test]
fn a_template_without_a_counter_is_not_numbered() {
    // The plain box numbers a multi-item rename; a template that placed no counter must not,
    // or a find/replace across twenty files would silently number them.
    let rule = RenameRule {
        find: "IMG_".into(),
        replace: String::new(),
        ..rule("{name}")
    };
    assert_eq!(
        vec!["0001.jpg", "0002.jpg"],
        names(
            &[file(r"C:\p\IMG_0001.jpg"), file(r"C:\p\IMG_0002.jpg")],
            &rule
        )
    );
}

// --- the date ---

#[test]
fn modified_defaults_to_a_sortable_date() {
    assert_eq!(
        "2026-03-09 report.txt",
        name(
            file_at(r"C:\p\report.txt", stamped()),
            &rule("{modified} {name}")
        )
    );
}

#[test]
fn modified_takes_a_format() {
    assert_eq!(
        "2026-03.txt",
        name(
            file_at(r"C:\p\report.txt", stamped()),
            &rule("{modified:yyyy-MM}{ext}")
        )
    );
}

#[test]
fn modified_on_an_item_with_no_date_is_reported_rather_than_stamped_year_one() {
    // Search results arrive without a timestamp until they are hydrated, and a flattened
    // search result is a perfectly ordinary place to rename from.
    let result = RenamePattern::apply(&[file(r"C:\p\report.txt")], &rule("{modified} {name}"));
    assert!(result[0].problem.is_some());
}

#[test]
fn modified_with_a_format_that_is_not_one_is_refused() {
    assert!(RenamePattern::validate_rule(&rule("{modified:%}")).is_some());
}

#[test]
fn modified_with_a_format_that_produces_slashes_says_so_instead_of_blaming_the_character() {
    // "d" is a valid format and gives 03/09/2026. The refusal that followed would have named
    // the slash rather than the format that put it there.
    let problem = RenamePattern::validate_rule(&rule("{modified:d}"));
    let problem = problem.expect("refused");
    assert!(problem.contains("yyyy-MM-dd"), "{problem}");
}

// --- refusals ---

#[test]
fn an_unknown_token_is_refused_and_says_how_to_type_a_literal_brace() {
    let problem = RenamePattern::validate_rule(&rule("{nmae}.txt")).expect("refused");
    assert!(problem.contains("{{"), "{problem}");
}

#[test]
fn unusable_templates_are_refused_with_a_reason() {
    for template in ["{name", "name}", "{name:x}", "{n:xyz}"] {
        let problem = RenamePattern::validate_rule(&rule(template));
        assert!(
            problem.is_some_and(|p| !p.trim().is_empty()),
            "{template:?} should be refused"
        );
    }
}

#[test]
fn a_literal_rule_is_never_judged_as_a_template() {
    assert_eq!(
        None,
        RenamePattern::validate_rule(&RenameRule::simple("{6B99A0C1}.tmp"))
    );
}

#[test]
fn a_good_rule_has_nothing_to_say() {
    let rule = RenameRule {
        find: "a".into(),
        replace: "b".into(),
        use_regex: true,
        ..rule("{base} {n:000}{ext}")
    };
    assert_eq!(None, RenamePattern::validate_rule(&rule));
}
