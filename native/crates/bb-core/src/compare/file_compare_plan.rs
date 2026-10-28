//! Deciding whether two files are worth diffing as text. Spec:
//! Core/Services/Compare/FileComparePlan.cs.
//!
//! A rule rather than an `if` in the window, so it can be tested, and so the answer to "why is
//! this showing hex?" is a function with a name rather than a condition buried in a view.

use bb_paths::file_name;

/// How a pair of files should be shown side by side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileCompareMode {
    /// A line diff.
    Text,
    /// A hex dump around the first differing byte.
    Binary,
}

/// What one side of a pair turned out to be, as far as anything can tell before reading.
/// `looks_like_text` is whether decoding the front of it produced convincing text, from the same
/// sniff the preview pane trusts, so the pane and the comparison cannot come to different
/// conclusions about what a file is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileCompareSide {
    /// Its file name, for the extension table.
    pub name: String,
    pub looks_like_text: bool,
}

impl FileCompareSide {
    pub fn new(name: impl Into<String>, looks_like_text: bool) -> FileCompareSide {
        FileCompareSide {
            name: name.into(),
            looks_like_text,
        }
    }
}

pub struct FileComparePlan;

impl FileComparePlan {
    /// [`FileCompareMode::Text`] only when *both* sides can be read as text.
    ///
    /// Both, not either. A `.txt` against a `.exe` diffed as text would render one side as a
    /// screen of mojibake and invite the reader to conclude something about it; the honest answer
    /// for a mixed pair is the one that works for the harder side. A side counts as text if its
    /// extension says so (the same table the preview pane trusts, so an empty `.cs` or one full of
    /// unusual characters is still code) or if, failing that, what was decoded is convincing on
    /// its own. That second rung is what makes an extensionless `README` or a `Dockerfile` work.
    pub fn mode_for(
        left: &FileCompareSide,
        right: &FileCompareSide,
        force_text: bool,
    ) -> FileCompareMode {
        if force_text {
            return FileCompareMode::Text;
        }

        if Self::is_text(left) && Self::is_text(right) {
            FileCompareMode::Text
        } else {
            FileCompareMode::Binary
        }
    }

    /// The preview classifier answers Text for a name with no extension at all, on the grounds
    /// that such a file is usually a Dockerfile or a LICENSE and reading one as text is harmless.
    /// That reasoning does not carry here: a wrong guess in a pane is one panel of mojibake the
    /// reader can dismiss, while a wrong guess here produces two of them side by side with
    /// differences marked between them, which reads as a finding. So the extensionless case falls
    /// through to what was actually decoded, and every other answer the classifier gives is used
    /// as it stands, so there is still one extension table.
    fn is_text(side: &FileCompareSide) -> bool {
        let named = !extension(&side.name).is_empty() && preview_stub::kind_for_is_text(&side.name);

        named || side.looks_like_text
    }
}

/// `Path.GetExtension`: the last dot of the leaf name onwards, or empty; a leading-dot name is
/// all extension.
fn extension(name: &str) -> &str {
    let leaf = file_name(name);
    match leaf.rfind('.') {
        Some(i) => &leaf[i..],
        None => "",
    }
}

/// The preview pane's extension table, as far as `FileComparePlan` asks it: whether
/// `PreviewClassifier.KindFor(name)` is `PreviewKind.Text`.
// TODO(merge): replaced by `crate::preview::PreviewClassifier::kind_for(name) == PreviewKind::Text`
// (with `SyntaxTokenizer::language_for` behind it) once the Preview area is ported. The tables are
// copied verbatim from Core/Services/Preview/PreviewClassifier.cs and SyntaxTokenizer.cs so the
// answer here is the one the pane gives.
mod preview_stub {
    const IMAGES: &str = ".png .jpg .jpeg .jfif .jpe .bmp .gif .tif .tiff .ico .wdp .jxr \
        .webp .heic .heif .avif .psd .tga .exr \
        .raw .cr2 .cr3 .nef .arw .dng .orf .rw2 .raf .pef .sr2 .srw .3fr .erf .kdc .mrw";

    const AUDIO: &str =
        ".mp3 .wav .flac .m4a .m4b .aac .ogg .oga .opus .wma .aif .aiff .ape .mid .midi";

    const VIDEO: &str = ".mp4 .mov .avi .mkv .wmv .webm .m4v .flv .mpg .mpeg .3gp .3g2 .m2ts \
        .mts .vob .ogv .asf .rm";

    const ARCHIVES: &str =
        ".zip .zipx .nupkg .snupkg .jar .war .aar .vsix .apk .whl .crx .epub .xpi .oxt";

    const FONTS: &str = ".ttf .otf .ttc .otc";

    const DOCUMENTS: &str = ".pdf .doc .docx .dot .dotx .xls .xlsx .xlsm .ppt .pptx .pps .ppsx \
        .odt .ods .odp .rtf .pub .vsd .vsdx .one .msg .eml .lnk .url .chm .xps .oxps";

    const TEXT_FILES: &str = ".txt .text .log .nfo .me .readme .lst .diff .patch .rej .orig \
        .md .markdown .mdx .rst .adoc .asciidoc .tex .bib .org \
        .json .jsonc .json5 .webmanifest .ipynb .yml .yaml .toml .ini .cfg .conf .properties .env \
        .xml .xaml .html .htm .xhtml .svg .csproj .fsproj .vbproj .sln .slnx .props .targets \
        .config .resx .nuspec .plist .csv .tsv .psv \
        .manifest .appxmanifest .xsd .xsl .xslt .dtd .rss .atom .opml .sitemap .axaml \
        .vcxproj .shproj .pubxml .ruleset .runsettings .wxs .wxi .storyboard \
        .cshtml .razor .vbhtml .aspx .ascx .ashx .asmx .master .jsp \
        .erb .hbs .mustache .twig .pug .ejs .liquid \
        .gitmodules .gitconfig .npmignore .prettierrc .eslintrc .babelrc .stylelintrc \
        .clang-format .htaccess .ignore .po .pot .strings .desktop .service .cue .ics .vcf \
        .f90 .for .pas .cr .rkt .scm .lisp .el .ahk .au3 .bas .vbs .wsf .applescript \
        .ninja .bzl .gn .gni .pro .in .ac .am \
        .css .scss .less .sass .styl \
        .js .mjs .cjs .jsx .ts .tsx .vue .svelte .astro \
        .cs .csx .fs .fsx .vb .c .h .cpp .hpp .cc .hh .cxx .m .mm .java .kt .kts .swift .go .rs \
        .php .rb .py .pyw .pyi .pl .pm .lua .r .jl .scala .dart .groovy .gradle .clj .cljs .ex .exs \
        .erl .hs .ml .mli .nim .zig .v .sv .vhd .asm .s .proto .graphql .gql .thrift \
        .sql .ps1 .psm1 .psd1 .sh .bash .zsh .fish .bat .cmd .awk .sed .vim \
        .tf .tfvars .hcl .bicep .nix .cmake .mk .make .dockerfile .bbs \
        .srt .vtt .sub .ass .reg .m3u .m3u8 .pls .gitignore .gitattributes .editorconfig .lock .sum .mod";

    /// `SyntaxTokenizer.ByExtension`: every extension with a syntax table.
    const SYNTAX_EXTENSIONS: &str = ".cs .csx .c .h .cpp .hpp .cc .hh .cxx .mm .java .kt .kts \
        .swift .go .rs .php .scala .dart .groovy .gradle .proto .zig .nim \
        .js .mjs .cjs .jsx .ts .tsx .json .jsonc .json5 .webmanifest .ipynb \
        .xml .xaml .html .htm .xhtml .svg .csproj .fsproj .vbproj .props .targets .config .resx \
        .nuspec .plist .manifest .appxmanifest .xsd .xsl .xslt .dtd .rss .atom .opml .sitemap \
        .axaml .vcxproj .shproj .pubxml .ruleset .runsettings .wxs .wxi .storyboard .cshtml .razor \
        .vbhtml .aspx .ascx .master .jsp .prettierrc .eslintrc .babelrc .stylelintrc .gitmodules \
        .gitconfig .npmignore .htaccess .css .scss .less .sql .py .pyw .pyi .ps1 .psm1 .psd1 \
        .sh .bash .zsh .bat .cmd .ini .cfg .conf .toml .yml .yaml .properties .md .markdown .mdx";

    /// `SyntaxTokenizer.WholeNames`: extensionless well-known names, and leading-dot files whose
    /// whole name is their extension.
    const SYNTAX_WHOLE_NAMES: &str = "Dockerfile Makefile Rakefile .gitignore .gitattributes \
        .dockerignore .editorconfig .npmrc .env";

    fn contains(table: &str, candidate: &str) -> bool {
        table
            .split_ascii_whitespace()
            .any(|e| e.eq_ignore_ascii_case(candidate))
    }

    /// `PreviewClassifier.KindFor(name) is PreviewKind.Text`.
    pub fn kind_for_is_text(name: &str) -> bool {
        let extension = super::extension(name);

        if extension.is_empty() {
            return true;
        }

        if contains(IMAGES, extension)
            || contains(AUDIO, extension)
            || contains(VIDEO, extension)
            || contains(ARCHIVES, extension)
            || contains(FONTS, extension)
            || contains(DOCUMENTS, extension)
        {
            return false;
        }
        if contains(TEXT_FILES, extension) {
            return true;
        }

        // A leading-dot file (".gitignore") has the whole name as its "extension".
        language_known(extension)
    }

    /// `SyntaxTokenizer.LanguageFor(name) != SyntaxLanguage.None`, for a name with an extension.
    fn language_known(extension: &str) -> bool {
        contains(SYNTAX_WHOLE_NAMES, extension) || contains(SYNTAX_EXTENSIONS, extension)
    }
}
