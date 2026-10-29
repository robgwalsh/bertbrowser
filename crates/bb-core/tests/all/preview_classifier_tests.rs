// Port of tests/BertBrowser.Core.Tests/PreviewClassifierTests.cs
use bb_core::attributes::{DIRECTORY, NORMAL, OFFLINE, RECALL_ON_DATA_ACCESS, RECALL_ON_OPEN};
use bb_core::preview::{
    PreviewClassifier, PreviewKind, PreviewMode, PreviewRefusal, PreviewRequest, PreviewTarget,
    SyntaxLanguage,
};

fn file(name: &str) -> PreviewTarget {
    sized(name, 1024)
}

fn sized(name: &str, size: i64) -> PreviewTarget {
    with(name, size, NORMAL)
}

fn with(name: &str, size: i64, attributes: u32) -> PreviewTarget {
    PreviewTarget {
        name: name.to_string(),
        size_bytes: size,
        attributes,
        is_directory: false,
    }
}

fn folder() -> PreviewTarget {
    PreviewTarget {
        name: "Pictures".to_string(),
        size_bytes: 0,
        attributes: DIRECTORY,
        is_directory: true,
    }
}

fn classify(target: &PreviewTarget) -> PreviewRequest {
    PreviewClassifier::classify_one(
        target,
        PreviewClassifier::DEFAULT_TEXT_BUDGET,
        PreviewMode::Auto,
    )
}

const PLACEHOLDER_ATTRIBUTES: [u32; 3] = [OFFLINE, RECALL_ON_OPEN, RECALL_ON_DATA_ACCESS];

// --- what each extension is ---

#[test]
fn kind_for_maps_the_extension() {
    for (name, expected) in [
        ("holiday.jpg", PreviewKind::Image),
        ("logo.PNG", PreviewKind::Image),
        ("shot.heic", PreviewKind::Image),
        ("raw.cr3", PreviewKind::Image),
        ("notes.txt", PreviewKind::Text),
        ("Program.cs", PreviewKind::Text),
        ("tsconfig.json", PreviewKind::Text),
        ("build.ps1", PreviewKind::Text),
        ("smoke.bbs", PreviewKind::Text),
        ("clip.mp4", PreviewKind::Media),
        ("song.flac", PreviewKind::Media),
        ("package.zip", PreviewKind::Archive),
        ("lib.nupkg", PreviewKind::Archive),
        ("Segoe.ttf", PreviewKind::Font),
        ("report.pdf", PreviewKind::Document),
        ("budget.xlsx", PreviewKind::Document),
    ] {
        assert_eq!(expected, PreviewClassifier::kind_for(name), "{name}");
    }
}

#[test]
fn kind_for_treats_an_unknown_extension_as_a_document() {
    // Not a refusal: the shell may well have a handler, and only it can say.
    assert_eq!(
        PreviewKind::Document,
        PreviewClassifier::kind_for("model.qqq")
    );
}

#[test]
fn kind_for_reads_the_obviously_textual_ones_as_text() {
    for name in [
        "choco.exe.manifest",
        "app.appxmanifest",
        "Schema.xsd",
        "Index.cshtml",
        "Native.vcxproj",
        "catalog.rss",
        "build.ninja",
        "messages.po",
    ] {
        assert_eq!(
            PreviewKind::Text,
            PreviewClassifier::kind_for(name),
            "{name}"
        );
    }
}

#[test]
fn a_document_carries_a_text_budget_for_the_fallback_when_the_shell_declines() {
    // The whole point of the fallback: an unrecognised file gets read rather than refused, once
    // the shell has had its turn and produced nothing.
    let plan = classify(&sized("mystery.qqq", 5000));
    assert_eq!(PreviewKind::Document, plan.kind);
    assert_eq!(5000, plan.byte_budget);
}

#[test]
fn an_enormous_image_downgraded_to_the_shell_carries_no_text_budget() {
    // "Too big to decode" is not an invitation to read a gigantic TIFF as text.
    assert_eq!(
        0,
        classify(&sized("scan.tiff", PreviewClassifier::MAX_IMAGE_BYTES + 1)).byte_budget
    );
}

#[test]
fn a_document_carries_its_language_too() {
    // So a file whose extension the syntax table knows still arrives coloured if the shell
    // declines and the text fallback takes over.
    assert_eq!(SyntaxLanguage::Xml, classify(&file("plugin.wxs")).language);
}

#[test]
fn kind_for_reads_an_extensionless_file_as_text() {
    assert_eq!(PreviewKind::Text, PreviewClassifier::kind_for("LICENSE"));
}

#[test]
fn kind_for_reads_a_leading_dot_file_as_text() {
    assert_eq!(PreviewKind::Text, PreviewClassifier::kind_for(".gitignore"));
}

#[test]
fn kind_for_keeps_office_containers_as_documents() {
    // A .docx really is a zip, but the shell makes a page-one thumbnail of it and a listing of
    // its guts helps nobody.
    assert_eq!(
        PreviewKind::Document,
        PreviewClassifier::kind_for("thesis.docx")
    );
}

#[test]
fn kind_for_reads_dot_ts_as_type_script_rather_than_transport_stream() {
    assert_eq!(PreviewKind::Text, PreviewClassifier::kind_for("app.ts"));
}

// --- refusals ---

#[test]
fn nothing_selected_is_refused() {
    let plan = PreviewClassifier::classify(
        &[],
        PreviewClassifier::DEFAULT_TEXT_BUDGET,
        PreviewMode::Auto,
    );
    assert_eq!(PreviewRefusal::NothingSelected, plan.refusal);
    assert_eq!(PreviewKind::None, plan.kind);
}

#[test]
fn several_selected_is_refused() {
    assert_eq!(
        PreviewRefusal::MultipleSelected,
        PreviewClassifier::classify(
            &[file("a.txt"), file("b.txt")],
            PreviewClassifier::DEFAULT_TEXT_BUDGET,
            PreviewMode::Auto
        )
        .refusal
    );
}

#[test]
fn a_folder_is_refused() {
    assert_eq!(PreviewRefusal::Folder, classify(&folder()).refusal);
}

#[test]
fn a_cloud_placeholder_is_refused_rather_than_downloaded() {
    // The whole point: reading it would make the provider fetch the content.
    for placeholder in PLACEHOLDER_ATTRIBUTES {
        assert_eq!(
            PreviewRefusal::NotDownloaded,
            classify(&with("holiday.jpg", 2048, NORMAL | placeholder)).refusal,
            "attribute {placeholder:#x}"
        );
    }
}

#[test]
fn a_placeholder_is_refused_even_for_a_kind_the_shell_would_handle() {
    assert_eq!(
        PreviewRefusal::NotDownloaded,
        classify(&with("report.pdf", 2048, RECALL_ON_DATA_ACCESS)).refusal
    );
}

#[test]
fn an_enormous_archive_is_refused() {
    assert_eq!(
        PreviewRefusal::TooLarge,
        classify(&sized(
            "everything.zip",
            PreviewClassifier::MAX_ARCHIVE_BYTES + 1
        ))
        .refusal
    );
}

#[test]
fn an_enormous_font_is_refused() {
    assert_eq!(
        PreviewRefusal::TooLarge,
        classify(&sized(
            "suspicious.ttf",
            PreviewClassifier::MAX_FONT_BYTES + 1
        ))
        .refusal
    );
}

#[test]
fn an_archive_just_under_the_cap_is_not_refused() {
    assert!(!classify(&sized("big.zip", PreviewClassifier::MAX_ARCHIVE_BYTES)).is_refused());
}

// --- budgets and downgrades ---

#[test]
fn an_enormous_image_falls_back_to_the_shell_rather_than_being_refused() {
    let plan = classify(&sized("scan.tiff", PreviewClassifier::MAX_IMAGE_BYTES + 1));
    assert!(!plan.is_refused());
    assert_eq!(PreviewKind::Document, plan.kind);
    assert_eq!(0, plan.byte_budget);
}

#[test]
fn a_huge_log_is_truncated_rather_than_refused() {
    let plan = classify(&sized("server.log", 40i64 << 30));
    assert_eq!(PreviewKind::Text, plan.kind);
    assert!(!plan.is_refused());
    assert_eq!(PreviewClassifier::DEFAULT_TEXT_BUDGET, plan.byte_budget);
}

#[test]
fn a_small_text_file_budgets_only_its_own_length() {
    assert_eq!(300, classify(&sized("notes.txt", 300)).byte_budget);
}

#[test]
fn the_text_budget_is_the_callers_to_set() {
    assert_eq!(
        64,
        PreviewClassifier::classify_one(&sized("notes.txt", 5000), 64, PreviewMode::Auto)
            .byte_budget
    );
}

#[test]
fn media_reads_nothing_itself() {
    // The shell streams it, and there is no sense in which a video is text.
    assert_eq!(0, classify(&sized("clip.mp4", 900_000_000)).byte_budget);
}

#[test]
fn a_text_plan_carries_its_language() {
    assert_eq!(
        SyntaxLanguage::CSharp,
        classify(&file("Program.cs")).language
    );
}

#[test]
fn a_plain_text_plan_carries_no_language() {
    assert_eq!(SyntaxLanguage::None, classify(&file("notes.txt")).language);
}

#[test]
fn one_selected_item_is_classified_as_itself() {
    assert_eq!(
        PreviewKind::Image,
        PreviewClassifier::classify(
            &[file("a.png")],
            PreviewClassifier::DEFAULT_TEXT_BUDGET,
            PreviewMode::Auto
        )
        .kind
    );
}

// --- forcing a mode ---

#[test]
fn hex_overrides_whatever_the_extension_claimed() {
    for name in ["holiday.jpg", "clip.mp4", "report.pdf", "Program.cs"] {
        assert_eq!(
            PreviewKind::Hex,
            PreviewClassifier::classify_one(
                &file(name),
                PreviewClassifier::DEFAULT_TEXT_BUDGET,
                PreviewMode::Hex
            )
            .kind,
            "{name}"
        );
    }
}

#[test]
fn a_forced_mode_still_costs_no_more_than_the_text_budget() {
    assert_eq!(
        64,
        PreviewClassifier::classify_one(&sized("clip.mp4", 900_000_000), 64, PreviewMode::Hex)
            .byte_budget
    );
}

#[test]
fn raw_carries_no_language_even_for_a_file_that_has_one() {
    // Raw means uncoloured, which is the whole difference between it and the ordinary text path
    // over a source file.
    assert_eq!(
        SyntaxLanguage::None,
        PreviewClassifier::classify_one(
            &file("Program.cs"),
            PreviewClassifier::DEFAULT_TEXT_BUDGET,
            PreviewMode::Text
        )
        .language
    );
}

#[test]
fn raw_is_still_text() {
    assert_eq!(
        PreviewKind::Text,
        PreviewClassifier::classify_one(
            &file("setup.exe"),
            PreviewClassifier::DEFAULT_TEXT_BUDGET,
            PreviewMode::Text
        )
        .kind
    );
}

#[test]
fn a_folder_is_still_refused_whatever_mode_was_forced() {
    for mode in [PreviewMode::Hex, PreviewMode::Text] {
        assert_eq!(
            PreviewRefusal::Folder,
            PreviewClassifier::classify_one(
                &folder(),
                PreviewClassifier::DEFAULT_TEXT_BUDGET,
                mode
            )
            .refusal,
            "{mode:?}"
        );
    }
}

#[test]
fn a_cloud_placeholder_is_still_refused_when_hex_is_forced() {
    // The rule the whole ordering exists for: forcing a mode says how to render bytes we were
    // already willing to read, never that we may now read bytes we refused. Move the override
    // above the refusals and this is a silent multi-gigabyte download.
    for placeholder in PLACEHOLDER_ATTRIBUTES {
        assert_eq!(
            PreviewRefusal::NotDownloaded,
            PreviewClassifier::classify_one(
                &with("holiday.jpg", 2048, NORMAL | placeholder),
                PreviewClassifier::DEFAULT_TEXT_BUDGET,
                PreviewMode::Hex
            )
            .refusal,
            "attribute {placeholder:#x}"
        );
    }
}

#[test]
fn a_forced_plan_says_which_mode_it_was_made_under() {
    // The build reads this rather than the view model, so a plan cannot be executed under a mode
    // it was not planned for.
    assert_eq!(
        PreviewMode::Hex,
        PreviewClassifier::classify_one(
            &file("a.png"),
            PreviewClassifier::DEFAULT_TEXT_BUDGET,
            PreviewMode::Hex
        )
        .mode
    );
}

#[test]
fn an_unforced_plan_is_auto() {
    assert_eq!(PreviewMode::Auto, classify(&file("a.png")).mode);
}
