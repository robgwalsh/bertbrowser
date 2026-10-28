// Port of tests/BertBrowser.Core.Tests/PreviewMetadataTests.cs
use bb_core::preview::{MetadataRow, PreviewKind, PreviewMetadata, ShellPropertyRow};

fn p(canonical: &str, name: &str, value: &str) -> ShellPropertyRow {
    ShellPropertyRow {
        canonical: canonical.to_string(),
        name: name.to_string(),
        value: value.to_string(),
    }
}

fn labels(rows: &[MetadataRow]) -> Vec<&str> {
    rows.iter().map(|r| r.label.as_str()).collect()
}

#[test]
fn image_properties_come_back_in_a_useful_order() {
    // Deliberately scrambled going in: the order out is the rule under test.
    let properties = [
        p("System.Photo.CameraModel", "Camera model", "X100V"),
        p("System.Image.Dimensions", "Dimensions", "6240 x 4160"),
        p("System.Photo.FNumber", "F-stop", "f/2"),
    ];

    let rows = PreviewMetadata::select(PreviewKind::Image, &properties);
    assert_eq!(vec!["Dimensions", "Camera model", "F-stop"], labels(&rows));
    assert_eq!("6240 x 4160", rows[0].value);
}

#[test]
fn a_key_we_do_not_care_about_is_dropped() {
    let properties = [
        p("System.Image.Dimensions", "Dimensions", "800 x 600"),
        p("System.FileAttributes", "Attributes", "A"),
        p(
            "System.ItemFolderPathDisplay",
            "Folder path",
            r"C:\Pictures",
        ),
    ];

    let rows = PreviewMetadata::select(PreviewKind::Image, &properties);
    assert_eq!(1, rows.len());
    assert_eq!("Dimensions", rows[0].label);
}

#[test]
fn selection_is_by_canonical_name_not_by_the_localised_label() {
    // The whole reason this class exists: on a German Windows the label is "Abmessungen" and
    // the canonical key is unchanged. Matching on the label would return nothing.
    let properties = [p("System.Image.Dimensions", "Abmessungen", "800 x 600")];

    let rows = PreviewMetadata::select(PreviewKind::Image, &properties);
    assert_eq!(1, rows.len());
    assert_eq!("Abmessungen", rows[0].label);
    assert_eq!("800 x 600", rows[0].value);
}

#[test]
fn audio_tags_lead_a_media_file() {
    let properties = [
        p("System.Audio.SampleRate", "Audio sample rate", "44100"),
        p("System.Music.Artist", "Contributing artists", "Someone"),
        p("System.Media.Duration", "Length", "00:03:41"),
    ];

    let rows = PreviewMetadata::select(PreviewKind::Media, &properties);
    assert_eq!(
        vec!["Contributing artists", "Length", "Audio sample rate"],
        labels(&rows)
    );
}

#[test]
fn an_empty_value_is_not_a_row() {
    assert!(
        PreviewMetadata::select(
            PreviewKind::Image,
            &[p("System.Image.Dimensions", "Dimensions", "")]
        )
        .is_empty()
    );
}

#[test]
fn a_property_with_no_canonical_name_is_dropped() {
    assert!(
        PreviewMetadata::select(PreviewKind::Image, &[p("", "Dimensions", "800 x 600")]).is_empty()
    );
}

#[test]
fn a_property_with_no_label_falls_back_to_the_tail_of_its_key() {
    let rows = PreviewMetadata::select(
        PreviewKind::Image,
        &[p("System.Image.Dimensions", "", "800 x 600")],
    );
    assert_eq!(1, rows.len());
    assert_eq!("Dimensions", rows[0].label);
}

#[test]
fn kinds_with_better_numbers_of_their_own_ask_the_shell_for_nothing() {
    for kind in [
        PreviewKind::Text,
        PreviewKind::Archive,
        PreviewKind::Font,
        PreviewKind::None,
    ] {
        assert!(
            PreviewMetadata::select(
                kind,
                &[p("System.Image.Dimensions", "Dimensions", "800 x 600")]
            )
            .is_empty(),
            "{kind:?}"
        );
    }
}

#[test]
fn the_strip_is_capped() {
    let properties: Vec<ShellPropertyRow> = [
        "System.Image.Dimensions",
        "System.Image.BitDepth",
        "System.Image.ColorSpace",
        "System.Photo.DateTaken",
        "System.Photo.CameraManufacturer",
        "System.Photo.CameraModel",
        "System.Photo.LensModel",
        "System.Photo.FNumber",
        "System.Photo.ExposureTime",
        "System.Photo.ISOSpeed",
        "System.Photo.FocalLength",
        "System.GPS.Latitude",
        "System.GPS.Longitude",
    ]
    .iter()
    .map(|canonical| p(canonical, canonical, "value"))
    .collect();

    assert_eq!(
        PreviewMetadata::MAX_ROWS,
        PreviewMetadata::select(PreviewKind::Image, &properties).len()
    );
}

#[test]
fn nothing_in_means_nothing_out() {
    assert!(PreviewMetadata::select(PreviewKind::Image, &[]).is_empty());
}
