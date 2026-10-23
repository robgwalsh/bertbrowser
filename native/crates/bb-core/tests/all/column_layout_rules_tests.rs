// Port of tests/BertBrowser.Core.Tests/ColumnLayoutRulesTests.cs.
use bb_core::columns::{
    ColumnCatalog, ColumnKind, ColumnLayoutRules, ColumnSetting, ResolvedColumn,
};

fn c(id: &str) -> ColumnSetting {
    cw(id, 100.0)
}

fn cw(id: &str, width: f64) -> ColumnSetting {
    ColumnSetting::new(id, width)
}

fn ids(columns: &[ResolvedColumn]) -> Vec<&str> {
    columns.iter().map(|c| c.id()).collect()
}

fn setting_ids(settings: &[ColumnSetting]) -> Vec<&str> {
    settings.iter().map(|c| c.id.as_str()).collect()
}

fn listing(user: Option<&[ColumnSetting]>) -> Vec<ResolvedColumn> {
    ColumnLayoutRules::resolve(user, false, false, false)
}

fn width_of(settings: &[ColumnSetting], id: &str) -> f64 {
    let found: Vec<&ColumnSetting> = settings.iter().filter(|c| c.id == id).collect();
    assert_eq!(1, found.len(), "exactly one {id}");
    found[0].width
}

// --- What a saved list resolves to ---

#[test]
fn never_configured_ships_todays_columns_at_todays_widths() {
    let columns = listing(None);

    assert_eq!(vec!["Name", "Size", "Type", "Modified"], ids(&columns));
    let widths: Vec<f64> = columns.iter().map(|c| c.width).collect();
    assert_eq!(vec![320.0, 110.0, 120.0, 140.0], widths);
}

#[test]
fn an_empty_list_is_honoured_rather_than_treated_as_unconfigured() {
    // Deliberately unlike NewFileTypes, where empty means "shipped defaults, all removed". Here
    // the Name rule below makes an empty layout a usable one, so empty can mean empty.
    let columns = listing(Some(&[]));

    assert_eq!(vec!["Name"], ids(&columns));
}

#[test]
fn name_is_put_back_when_a_saved_list_has_lost_it() {
    assert_eq!(vec!["Name", "Size"], ids(&listing(Some(&[c("Size")]))));
}

#[test]
fn name_is_moved_to_the_front_when_it_is_not_first() {
    assert_eq!(
        vec!["Name", "Size", "Modified"],
        ids(&listing(Some(&[c("Size"), c("Modified"), c("Name")])))
    );
}

#[test]
fn a_duplicate_collapses_to_its_first_occurrence_keeping_that_width() {
    let columns = listing(Some(&[c("Name"), cw("Size", 200.0), cw("Size", 900.0)]));

    assert_eq!(vec!["Name", "Size"], ids(&columns));
    assert_eq!(200.0, columns[1].width);
}

#[test]
fn the_column_count_is_capped() {
    // Every curated property, several times over — a hand-edited file should not be able to make
    // a row realize hundreds of cells.
    let curated = ColumnCatalog::curated();
    let mut many: Vec<ColumnSetting> = curated.iter().map(|s| c(&s.id)).collect();
    many.extend(
        curated
            .iter()
            .cycle()
            .take(curated.len() * 4)
            .enumerate()
            .map(|(i, s)| ColumnSetting::new(&format!("{}{i}", s.id), 100.0)),
    );

    assert!(listing(Some(&many)).len() <= ColumnLayoutRules::MAX_COLUMNS);
}

// --- Which ids survive ---

#[test]
fn an_unknown_built_in_id_is_dropped() {
    // A bare word this build does not know came from a newer one and names a column it cannot
    // render. Keeping it would mean a blank column nobody can explain or remove.
    assert_eq!(
        vec!["Name", "Size"],
        ids(&listing(Some(&[c("Name"), c("Colour"), c("Size")])))
    );
}

#[test]
fn an_unrecognised_canonical_name_is_kept_and_renders_blank() {
    // The other half of the same rule: this machine may simply have no handler for it, and
    // unknown is blank, never wrong.
    let columns = listing(Some(&[c("Name"), c("System.Contact.NickName")]));

    assert_eq!(vec!["Name", "System.Contact.NickName"], ids(&columns));
    assert_eq!(ColumnKind::ShellProperty, columns[1].spec.kind);
}

#[test]
fn an_injected_column_in_a_saved_list_is_ignored() {
    // Folder and Match follow the list's mode. One arriving from a hand-edited file would
    // otherwise show up twice, or show up in a listing where it means nothing.
    for id in ["RelativePath", "Match"] {
        assert_eq!(
            vec!["Name", "Size"],
            ids(&listing(Some(&[c("Name"), c(id), c("Size")]))),
            "{id}"
        );
    }
}

// --- Widths ---

#[test]
fn an_unusable_width_becomes_the_column_default() {
    let rows = [
        (f64::NAN, "what a gripper double-click leaves behind"),
        (f64::INFINITY, "infinity"),
        (0.0, "zero"),
        (-40.0, "negative"),
    ];
    for (width, why) in rows {
        assert_eq!(
            110.0,
            listing(Some(&[c("Name"), cw("Size", width)]))[1].width,
            "{width}: {why}"
        );
    }
}

#[test]
fn a_width_saved_on_a_much_wider_monitor_is_clamped() {
    assert_eq!(
        ColumnLayoutRules::MAX_WIDTH,
        listing(Some(&[c("Name"), cw("Size", 99_999.0)]))[1].width
    );
}

#[test]
fn a_width_too_narrow_to_grab_is_clamped() {
    assert_eq!(
        ColumnLayoutRules::MIN_WIDTH,
        listing(Some(&[c("Name"), cw("Size", 1.0)]))[1].width
    );
}

// --- The two injected columns ---

#[test]
fn folder_appears_only_in_a_flattened_list() {
    assert!(!ids(&listing(None)).contains(&"RelativePath"));
    assert!(ids(&ColumnLayoutRules::resolve(None, true, false, false)).contains(&"RelativePath"));
}

#[test]
fn match_is_keyed_on_content_matches_and_not_on_flattening() {
    // Every search flattens, so keying Match on that would make an empty column appear the
    // moment anyone typed into the box — which reads as a rendering fault.
    let searched = ColumnLayoutRules::resolve(None, true, false, false);
    assert!(!ids(&searched).contains(&"Match"));

    let grepped = ColumnLayoutRules::resolve(None, true, true, false);
    assert!(ids(&grepped).contains(&"Match"));
}

#[test]
fn folder_and_match_sit_between_the_name_and_everything_else() {
    assert_eq!(
        vec!["Name", "RelativePath", "Match", "Size", "Type", "Modified"],
        ids(&ColumnLayoutRules::resolve(None, true, true, false))
    );
}

#[test]
fn status_appears_only_while_two_folders_are_being_compared() {
    assert!(!ids(&listing(None)).contains(&"CompareStatus"));
    assert!(ids(&ColumnLayoutRules::resolve(None, false, false, true)).contains(&"CompareStatus"));
}

/// At the end, unlike Folder and Match. It answers a question about the row rather than saying
/// what the row is, and putting it first would shove the columns someone arranged for this
/// folder along by one every time a comparison started.
#[test]
fn status_goes_after_everything_else() {
    let columns = ColumnLayoutRules::resolve(None, false, false, true);

    assert_eq!(Some(&"CompareStatus"), ids(&columns).last());
}

#[test]
fn status_is_never_persisted_or_offered_on_the_header_menu() {
    assert!(ColumnCatalog::is_injected(ColumnCatalog::COMPARE_STATUS));
}

#[test]
fn the_injected_columns_are_marked_as_such() {
    let columns = ColumnLayoutRules::resolve(None, true, true, false);

    for column in &columns {
        assert_eq!(
            ColumnCatalog::is_injected(column.id()),
            column.injected,
            "{}",
            column.id()
        );
    }
}

#[test]
fn match_is_not_sortable() {
    // Sorting a result set by the line number a needle was found on means nothing across
    // different files. This used to be said by giving that column no Tag to parse.
    let columns = ColumnLayoutRules::resolve(None, true, true, false);
    let matches: Vec<&ResolvedColumn> = columns.iter().filter(|c| c.id() == "Match").collect();
    assert_eq!(1, matches.len());

    assert!(!matches[0].spec.sortable);
}

// --- Editing ---

#[test]
fn toggling_a_column_on_adds_it_at_the_end() {
    assert_eq!(
        vec!["Name", "Size", "Type", "Modified", "System.Photo.DateTaken"],
        setting_ids(&ColumnLayoutRules::toggle(
            None,
            "System.Photo.DateTaken",
            true
        ))
    );
}

#[test]
fn toggling_a_column_on_twice_adds_it_once() {
    let once = ColumnLayoutRules::toggle(None, "Created", true);
    let twice = ColumnLayoutRules::toggle(Some(&once), "Created", true);
    assert_eq!(once.len(), twice.len());
}

#[test]
fn toggling_a_column_off_removes_it() {
    assert_eq!(
        vec!["Name", "Size", "Modified"],
        setting_ids(&ColumnLayoutRules::toggle(None, "Type", false))
    );
}

#[test]
fn name_cannot_be_switched_off() {
    assert!(setting_ids(&ColumnLayoutRules::toggle(None, "Name", false)).contains(&"Name"));
}

#[test]
fn moving_a_column_reorders_it() {
    assert_eq!(
        vec!["Name", "Modified", "Size", "Type"],
        setting_ids(&ColumnLayoutRules::move_to(None, "Modified", 1))
    );
}

#[test]
fn nothing_can_be_moved_in_front_of_name() {
    assert_eq!(
        "Name",
        ColumnLayoutRules::move_to(None, "Modified", 0)[0].id
    );
}

#[test]
fn name_cannot_be_moved_away_from_the_front() {
    // AllowsColumnReorder has always let the header be dragged; persisting the layout is what
    // would have made that stick.
    assert_eq!("Name", ColumnLayoutRules::move_to(None, "Name", 3)[0].id);
}

#[test]
fn an_out_of_range_move_clamps_rather_than_throwing() {
    assert_eq!(
        vec!["Name", "Type", "Modified", "Size"],
        setting_ids(&ColumnLayoutRules::move_to(None, "Size", 99))
    );
}

#[test]
fn moving_a_column_that_is_not_there_changes_nothing() {
    assert_eq!(
        vec!["Name", "Size", "Type", "Modified"],
        setting_ids(&ColumnLayoutRules::move_to(None, "Created", 1))
    );
}

#[test]
fn setting_a_width_sanitizes_it() {
    assert_eq!(
        110.0,
        width_of(
            &ColumnLayoutRules::set_width(None, "Size", f64::NAN),
            "Size"
        )
    );
}

// --- The "More columns…" picker ---

#[test]
fn picking_a_property_adds_it_at_the_end() {
    assert_eq!(
        vec!["Name", "Size", "Type", "Modified", "System.Photo.DateTaken"],
        setting_ids(&ColumnLayoutRules::apply_picked(
            None,
            &["System.Photo.DateTaken"]
        ))
    );
}

#[test]
fn unticking_a_property_removes_it() {
    let with_one = ColumnLayoutRules::apply_picked(None, &["System.Photo.DateTaken"]);

    assert_eq!(
        vec!["Name", "Size", "Type", "Modified"],
        setting_ids(&ColumnLayoutRules::apply_picked::<&str>(
            Some(&with_one),
            &[]
        ))
    );
}

#[test]
fn opening_the_picker_and_changing_nothing_changes_nothing() {
    // What the rule is really for: the answer contains everything already ticked, so a naive
    // apply would remove and re-add each one and quietly shuffle them all to the end.
    let before = [
        cw("Name", 400.0),
        cw("System.Photo.DateTaken", 150.0),
        cw("Size", 90.0),
    ];

    let after = ColumnLayoutRules::apply_picked(Some(&before), &["System.Photo.DateTaken"]);

    assert_eq!(setting_ids(&before), setting_ids(&after));
    assert_eq!(150.0, width_of(&after, "System.Photo.DateTaken"));
}

#[test]
fn the_picker_leaves_the_built_in_columns_alone() {
    // It lists the property system, so a built-in missing from its answer means "not offered
    // here", never "remove it".
    assert_eq!(
        vec!["Name", "Size", "Type", "Modified"],
        setting_ids(&ColumnLayoutRules::apply_picked::<&str>(None, &[]))
    );
}

// --- Capturing a drag ---

#[test]
fn capture_order_takes_the_live_order_and_drops_the_injected_columns() {
    let live = ["Name", "RelativePath", "Match", "Modified", "Size", "Type"];

    assert_eq!(
        vec!["Name", "Modified", "Size", "Type"],
        setting_ids(&ColumnLayoutRules::capture_order(&live, None))
    );
}

#[test]
fn capture_order_keeps_the_widths_the_columns_already_had() {
    let saved = [cw("Name", 400.0), cw("Size", 200.0)];
    let live = ["Size", "Name"];

    let captured = ColumnLayoutRules::capture_order(&live, Some(&saved));

    assert_eq!(400.0, width_of(&captured, "Name"));
    assert_eq!(200.0, width_of(&captured, "Size"));
}

#[test]
fn capture_order_snaps_a_dragged_name_column_back_to_the_front() {
    assert_eq!(
        vec!["Name", "Size", "Type"],
        setting_ids(&ColumnLayoutRules::capture_order(
            &["Size", "Type", "Name"],
            None
        ))
    );
}

// --- Stepping a width with the wheel ---

#[test]
fn a_wheel_notch_moves_the_width_by_ten() {
    assert_eq!(150.0, ColumnLayoutRules::step_width(140.0, 1, false));
    assert_eq!(130.0, ColumnLayoutRules::step_width(140.0, -1, false));
}

#[test]
fn each_notch_of_one_event_counts() {
    assert_eq!(170.0, ColumnLayoutRules::step_width(140.0, 3, false));
}

#[test]
fn a_coarse_notch_snaps_an_off_grid_width_onto_the_grid() {
    // 137 up is 140, not 147. The point of a coarse step is that spinning the wheel walks a
    // tidy sequence instead of carrying an arbitrary dragged width along with it forever.
    assert_eq!(140.0, ColumnLayoutRules::step_width(137.0, 1, false));
    assert_eq!(130.0, ColumnLayoutRules::step_width(137.0, -1, false));
}

#[test]
fn a_fine_notch_moves_by_one_and_does_not_snap() {
    assert_eq!(138.0, ColumnLayoutRules::step_width(137.0, 1, true));
    assert_eq!(136.0, ColumnLayoutRules::step_width(137.0, -1, true));
}

#[test]
fn stepping_stops_at_the_bounds_a_dragged_gripper_stops_at() {
    assert_eq!(
        ColumnLayoutRules::MIN_WIDTH,
        ColumnLayoutRules::step_width(ColumnLayoutRules::MIN_WIDTH, -1, false)
    );
    assert_eq!(
        ColumnLayoutRules::MAX_WIDTH,
        ColumnLayoutRules::step_width(ColumnLayoutRules::MAX_WIDTH, 1, false)
    );
}

#[test]
fn stepping_an_unusable_width_repairs_it_rather_than_propagating_it() {
    // NaN is what double-clicking a gripper leaves behind, and it is a real saved value. The
    // wheel must not turn it into NaN + 10.
    assert_eq!(110.0, ColumnLayoutRules::step_width(f64::NAN, -1, false));
    assert_eq!(
        130.0,
        ColumnLayoutRules::step_width(f64::INFINITY, 1, false)
    );
}

#[test]
fn no_notches_leaves_the_width_alone_but_still_repairs_it() {
    assert_eq!(137.0, ColumnLayoutRules::step_width(137.0, 0, false));
    assert_eq!(120.0, ColumnLayoutRules::step_width(f64::NAN, 0, false));
}
