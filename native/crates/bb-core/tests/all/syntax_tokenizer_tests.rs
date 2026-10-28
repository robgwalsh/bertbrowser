// Port of tests/BertBrowser.Core.Tests/SyntaxTokenizerTests.cs
use bb_core::preview::{SyntaxClass, SyntaxLanguage, SyntaxSpan, SyntaxTokenizer};

// --- picking a language ---

#[test]
fn language_for_picks_the_table() {
    for (name, expected) in [
        ("Program.cs", SyntaxLanguage::CSharp),
        ("main.rs", SyntaxLanguage::CFamily),
        ("app.tsx", SyntaxLanguage::JavaScript),
        ("tsconfig.json", SyntaxLanguage::Json),
        ("MainWindow.xaml", SyntaxLanguage::Xml),
        ("site.scss", SyntaxLanguage::Css),
        ("schema.SQL", SyntaxLanguage::Sql),
        ("build.ps1", SyntaxLanguage::PowerShell),
        ("install.sh", SyntaxLanguage::Shell),
        ("setup.py", SyntaxLanguage::Python),
        ("README.md", SyntaxLanguage::Markdown),
        ("docker-compose.yml", SyntaxLanguage::Ini),
        ("Dockerfile", SyntaxLanguage::Shell),
        (".gitignore", SyntaxLanguage::Ini),
        ("notes.txt", SyntaxLanguage::None),
        ("LICENSE", SyntaxLanguage::None),
    ] {
        assert_eq!(expected, SyntaxTokenizer::language_for(name), "{name}");
    }
}

// --- the cover property, which is the one that must never break ---

/// Deliberately nasty: unterminated everything, in every order.
const ADVERSARIAL: &str = concat!(
    "\"unterminated string\nvalue = 'it\\'s' /* block\n#comment <tag attr=\"v\"> 0x1F 3.14e9\n",
    "// line\n<!-- xml comment\n``` fence\n- bullet\n> quote\n### heading\n-- sql\n<# ps #>\n\"\"\"\n$var @attr\n"
);

#[test]
fn spans_cover_the_text_exactly_once() {
    for language in SyntaxLanguage::ALL {
        let spans = SyntaxTokenizer::tokenize(ADVERSARIAL, language);
        let mut covered = 0;
        for span in &spans {
            assert!(
                span.length > 0,
                "{language:?}: an empty span reached the view"
            );
            assert_eq!(covered, span.start, "{language:?}");
            covered += span.length;
        }
        assert_eq!(ADVERSARIAL.chars().count(), covered, "{language:?}");
    }
}

#[test]
fn empty_text_yields_no_spans() {
    for language in SyntaxLanguage::ALL {
        assert!(
            SyntaxTokenizer::tokenize("", language).is_empty(),
            "{language:?}"
        );
    }
}

#[test]
fn an_unknown_language_is_one_plain_span() {
    let spans = SyntaxTokenizer::tokenize("anything at all", SyntaxLanguage::None);
    assert_eq!(vec![SyntaxSpan::new(0, 15, SyntaxClass::Text)], spans);
}

// --- classification ---

fn class_at(text: &str, language: SyntaxLanguage, index: usize) -> SyntaxClass {
    for span in SyntaxTokenizer::tokenize(text, language) {
        if index >= span.start && index < span.start + span.length {
            return span.class;
        }
    }
    panic!("no span covers index {index}");
}

#[test]
fn a_keyword_is_a_keyword() {
    assert_eq!(
        SyntaxClass::Keyword,
        class_at("public void Go()", SyntaxLanguage::CSharp, 0)
    );
}

#[test]
fn an_ordinary_identifier_is_not() {
    assert_eq!(
        SyntaxClass::Text,
        class_at("public void Go()", SyntaxLanguage::CSharp, 12)
    );
}

#[test]
fn a_comment_marker_inside_a_string_does_not_start_a_comment() {
    // The classic tokenizer bug: everything after this would go grey.
    let code = "var url = \"http://example.com\"; var x = 1;";
    assert_eq!(
        SyntaxClass::String,
        class_at(code, SyntaxLanguage::CSharp, 20)
    );
    assert_eq!(
        SyntaxClass::Keyword,
        class_at(code, SyntaxLanguage::CSharp, 32)
    ); // the second 'var'
}

#[test]
fn a_quote_inside_a_comment_does_not_start_a_string() {
    let code = "// it's fine\nint x = 1;";
    assert_eq!(
        SyntaxClass::Comment,
        class_at(code, SyntaxLanguage::CSharp, 5)
    );
    assert_eq!(
        SyntaxClass::Keyword,
        class_at(code, SyntaxLanguage::CSharp, 13)
    );
}

#[test]
fn an_escaped_quote_does_not_end_the_string() {
    let code = "\"a\\\"b\" + tail";
    assert_eq!(
        SyntaxClass::String,
        class_at(code, SyntaxLanguage::CSharp, 3)
    );
    assert_eq!(SyntaxClass::Text, class_at(code, SyntaxLanguage::CSharp, 9));
}

#[test]
fn an_unterminated_block_comment_runs_to_the_end() {
    let code = "int x;\n/* never closed\nmore text";
    assert_eq!(
        SyntaxClass::Comment,
        class_at(code, SyntaxLanguage::CSharp, code.len() - 1)
    );
}

#[test]
fn an_unterminated_string_stops_at_the_end_of_its_line() {
    // Otherwise one stray apostrophe greys out the rest of the file.
    let code = "value = 'oops\nint x = 1;";
    assert_eq!(
        SyntaxClass::Keyword,
        class_at(code, SyntaxLanguage::CSharp, 14)
    );
}

#[test]
fn numbers_are_numbers() {
    assert_eq!(
        SyntaxClass::Number,
        class_at("x = 0xDEADBEEF;", SyntaxLanguage::CSharp, 6)
    );
    assert_eq!(
        SyntaxClass::Number,
        class_at("x = 3.14e9;", SyntaxLanguage::CSharp, 8)
    );
}

#[test]
fn sql_keywords_are_recognised_in_either_case() {
    assert_eq!(
        SyntaxClass::Keyword,
        class_at("SELECT * FROM t", SyntaxLanguage::Sql, 0)
    );
    assert_eq!(
        SyntaxClass::Keyword,
        class_at("select * from t", SyntaxLanguage::Sql, 0)
    );
}

#[test]
fn sql_uses_double_dash_for_comments() {
    assert_eq!(
        SyntaxClass::Comment,
        class_at("-- a note\nSELECT 1", SyntaxLanguage::Sql, 4)
    );
}

#[test]
fn xml_colours_the_element_name_and_the_attribute_value() {
    let markup = "<Button Content=\"Go\"/>";
    assert_eq!(
        SyntaxClass::Punctuation,
        class_at(markup, SyntaxLanguage::Xml, 0)
    );
    assert_eq!(
        SyntaxClass::Keyword,
        class_at(markup, SyntaxLanguage::Xml, 3)
    );
    assert_eq!(SyntaxClass::Text, class_at(markup, SyntaxLanguage::Xml, 9));
    assert_eq!(
        SyntaxClass::String,
        class_at(markup, SyntaxLanguage::Xml, 17)
    );
}

#[test]
fn xml_comments_are_comments() {
    assert_eq!(
        SyntaxClass::Comment,
        class_at("<!-- hidden --><b/>", SyntaxLanguage::Xml, 6)
    );
}

#[test]
fn markdown_headings_and_fences_are_distinguished() {
    let doc = "# Title\ntext\n```\ncode\n```\n";
    assert_eq!(
        SyntaxClass::Keyword,
        class_at(doc, SyntaxLanguage::Markdown, 2)
    );
    assert_eq!(
        SyntaxClass::Text,
        class_at(doc, SyntaxLanguage::Markdown, 9)
    );
    assert_eq!(
        SyntaxClass::String,
        class_at(doc, SyntaxLanguage::Markdown, 18)
    ); // inside the fence
}

#[test]
fn power_shell_uses_its_own_block_comment() {
    assert_eq!(
        SyntaxClass::Comment,
        class_at("<# note #>\nif ($x) {}", SyntaxLanguage::PowerShell, 4)
    );
}

#[test]
fn json_colours_its_literals() {
    let json = "{ \"on\": true }";
    assert_eq!(SyntaxClass::String, class_at(json, SyntaxLanguage::Json, 3));
    assert_eq!(
        SyntaxClass::Keyword,
        class_at(json, SyntaxLanguage::Json, 9)
    );
}

#[test]
fn adjacent_runs_of_the_same_class_are_merged() {
    // Neither word is a keyword, and a space is neither punctuation nor part of an identifier —
    // so the line arrives as one span rather than three.
    let spans = SyntaxTokenizer::tokenize("abc def", SyntaxLanguage::CSharp);
    assert_eq!(vec![SyntaxSpan::new(0, 7, SyntaxClass::Text)], spans);
}
