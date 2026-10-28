//! Decides what the preview pane will do with a selection, touching nothing. This is the planner
//! of the preview feature: every rule that can be settled without opening the file lives here, so
//! the rules are unit-tested rather than inferred from what the pane happens to render.
//!
//! The two decisions worth understanding are the ones that differ from Explorer. A cloud
//! placeholder is `PreviewRefusal::NotDownloaded` rather than a silent multi-gigabyte download;
//! and an unrecognised extension becomes `PreviewKind::Document`, an honest attempt through the
//! shell, rather than an immediate refusal, because the classifier cannot know which preview
//! handlers this machine has. Spec: Core/Services/Preview/PreviewClassifier.cs.

use std::collections::HashSet;
use std::sync::LazyLock;

use super::{
    PreviewKind, PreviewMode, PreviewRefusal, PreviewRequest, PreviewTarget, SyntaxLanguage,
    SyntaxTokenizer,
};
use crate::attributes::{OFFLINE, RECALL_ON_DATA_ACCESS, RECALL_ON_OPEN};

pub struct PreviewClassifier;

impl PreviewClassifier {
    /// How much of a text file is read by default. Enough for any source file; small enough that
    /// a stray multi-gigabyte log costs a millisecond rather than the session.
    pub const DEFAULT_TEXT_BUDGET: i64 = 1 << 20; // 1 MB

    /// Above this an image is not decoded here; the shell is asked for a preview instead, which
    /// it can produce without materialising the whole bitmap.
    pub const MAX_IMAGE_BYTES: i64 = 64 << 20; // 64 MB

    /// Above this an archive's directory is not read. Nothing streams a listing out of a 2 GB
    /// container fast enough to be worth doing while someone arrows down a list.
    pub const MAX_ARCHIVE_BYTES: i64 = 2 << 30; // 2 GB

    /// Above this a font file is refused: a real one is a few hundred kilobytes, so anything
    /// past this is not a font we want to hand to the text stack.
    pub const MAX_FONT_BYTES: i64 = 32 << 20; // 32 MB

    /// Cloud-placeholder attributes. Reading a file carrying any of these makes the provider
    /// fetch its content, which is never something a preview may decide on its own.
    pub const PLACEHOLDER: u32 = OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS;

    /// Whether reading this file's bytes would make a sync provider download it.
    ///
    /// One predicate rather than the mask repeated, because the preview pane is not the only
    /// thing that must not trip it: a shell-metadata column opens files too (`GPS_OPENSLOWITEM`),
    /// and one scrolled past a synced photo folder would pull the whole thing down a row at a
    /// time. Two copies of this test could disagree; one cannot.
    pub fn is_cloud_placeholder(attributes: u32) -> bool {
        attributes & Self::PLACEHOLDER != 0
    }

    /// The plan for a whole selection: nothing and several are refused; one is classified.
    pub fn classify(
        selection: &[PreviewTarget],
        text_budget: i64,
        mode: PreviewMode,
    ) -> PreviewRequest {
        match selection {
            [] => refuse(PreviewRefusal::NothingSelected),
            [one] => Self::classify_one(one, text_budget, mode),
            _ => refuse(PreviewRefusal::MultipleSelected),
        }
    }

    pub fn classify_one(
        target: &PreviewTarget,
        text_budget: i64,
        mode: PreviewMode,
    ) -> PreviewRequest {
        if target.is_directory {
            return refuse(PreviewRefusal::Folder);
        }

        if Self::is_cloud_placeholder(target.attributes) {
            return refuse(PreviewRefusal::NotDownloaded);
        }

        let capped = target.size_bytes.min(text_budget.max(0));

        // The override comes after the two refusals above and never before them. Forcing a mode
        // says how to render bytes we were already willing to read; it is not permission to read
        // bytes we refused, and a placeholder refused here is a download that does not happen.
        // Neither forced mode carries a language: raw means uncoloured, which is the whole
        // difference between it and the ordinary text path over a .cs.
        if matches!(mode, PreviewMode::Hex | PreviewMode::Text) {
            return PreviewRequest {
                kind: if mode == PreviewMode::Hex {
                    PreviewKind::Hex
                } else {
                    PreviewKind::Text
                },
                refusal: PreviewRefusal::None,
                byte_budget: capped,
                language: SyntaxLanguage::None,
                mode,
            };
        }

        let kind = Self::kind_for(&target.name);
        match kind {
            // Too big to decode ourselves is not too big to preview: the shell's handler can
            // produce a thumbnail of a 500 MB TIFF without us reading one byte of it. A zero
            // budget, unlike an ordinary document's: this is still an image, and reading a
            // gigantic TIFF as text if the shell declines would be nonsense.
            PreviewKind::Image if target.size_bytes > Self::MAX_IMAGE_BYTES => {
                plan(PreviewKind::Document, 0, SyntaxLanguage::None)
            }

            PreviewKind::Archive if target.size_bytes > Self::MAX_ARCHIVE_BYTES => {
                refuse(PreviewRefusal::TooLarge)
            }
            PreviewKind::Font if target.size_bytes > Self::MAX_FONT_BYTES => {
                refuse(PreviewRefusal::TooLarge)
            }

            // Text is never refused for size: it is truncated, and the pane says so.
            PreviewKind::Text => plan(
                PreviewKind::Text,
                capped,
                SyntaxTokenizer::language_for(&target.name),
            ),

            PreviewKind::Image | PreviewKind::Archive | PreviewKind::Font => {
                plan(kind, target.size_bytes, SyntaxLanguage::None)
            }

            // A document carries a text budget even though the shell is asked first: it is what
            // the text fallback may read when the shell turns out to have no preview for it.
            // That fallback is the answer to an extension table's endless tail (a `.manifest`, a
            // config file with a name nobody standardised, an extensionless script), and it is
            // exactly where Explorer's pane gives up.
            PreviewKind::Document => {
                plan(kind, capped, SyntaxTokenizer::language_for(&target.name))
            }

            // Media is the shell's to stream, and there is nothing to read as text.
            _ => plan(kind, 0, SyntaxLanguage::None),
        }
    }

    /// The extension table. Anything unlisted is `PreviewKind::Document`: the shell gets a try,
    /// and only its refusal produces "no preview available".
    pub fn kind_for(name: &str) -> PreviewKind {
        let extension = extension_of(name);

        // An extensionless file is a source file far more often than it is anything else
        // (Dockerfile, Makefile, LICENSE, hosts), and reading one as text is harmless where
        // reading it as a document is useless.
        if extension.is_empty() {
            return PreviewKind::Text;
        }

        let key = extension.to_lowercase();
        if IMAGES.contains(key.as_str()) {
            return PreviewKind::Image;
        }
        if AUDIO.contains(key.as_str()) || VIDEO.contains(key.as_str()) {
            return PreviewKind::Media;
        }
        if ARCHIVES.contains(key.as_str()) {
            return PreviewKind::Archive;
        }
        if FONTS.contains(key.as_str()) {
            return PreviewKind::Font;
        }
        if DOCUMENTS.contains(key.as_str()) {
            return PreviewKind::Document;
        }
        if TEXT_FILES.contains(key.as_str()) {
            return PreviewKind::Text;
        }

        // A leading-dot file (".gitignore") has the whole name as its "extension".
        if SyntaxTokenizer::language_for(name) != SyntaxLanguage::None {
            return PreviewKind::Text;
        }

        PreviewKind::Document
    }

    /// Whether a `PreviewKind::Media` name is a video rather than audio: what `MediaPlaylist`
    /// keeps a run of "next" within.
    pub fn is_video(name: &str) -> bool {
        VIDEO.contains(extension_of(name).to_lowercase().as_str())
    }
}

fn refuse(refusal: PreviewRefusal) -> PreviewRequest {
    PreviewRequest {
        kind: PreviewKind::None,
        refusal,
        byte_budget: 0,
        language: SyntaxLanguage::None,
        mode: PreviewMode::Auto,
    }
}

fn plan(kind: PreviewKind, byte_budget: i64, language: SyntaxLanguage) -> PreviewRequest {
    PreviewRequest {
        kind,
        refusal: PreviewRefusal::None,
        byte_budget,
        language,
        mode: PreviewMode::Auto,
    }
}

/// `Path.GetExtension`: from the last dot of the file name, inclusive; empty when there is none
/// or it is the last character. A leading-dot name's "extension" is the whole name.
pub(crate) fn extension_of(name: &str) -> &str {
    let file_name = bb_paths::file_name(name);
    match file_name.rfind('.') {
        Some(dot) if dot + 1 < file_name.len() => &file_name[dot..],
        _ => "",
    }
}

fn set(extensions: &'static str) -> HashSet<&'static str> {
    extensions.split_ascii_whitespace().collect()
}

// WIC decodes the first group directly; the rest need a codec or a shell handler, and fall back to
// the shell preview when one is missing. Both are Image either way: the pane tries the decoder
// first and the shell second, so the table does not have to know what is installed.
static IMAGES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    set(
        ".png .jpg .jpeg .jfif .jpe .bmp .gif .tif .tiff .ico .wdp .jxr \
         .webp .heic .heif .avif .psd .tga .exr \
         .raw .cr2 .cr3 .nef .arw .dng .orf .rw2 .raf .pef .sr2 .srw .3fr .erf .kdc .mrw",
    )
});

static AUDIO: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    set(".mp3 .wav .flac .m4a .m4b .aac .ogg .oga .opus .wma .aif .aiff .ape .mid .midi")
});

static VIDEO: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    set(
        ".mp4 .mov .avi .mkv .wmv .webm .m4v .flv .mpg .mpeg .3gp .3g2 .m2ts .mts .vob .ogv .asf .rm",
    )
});

// Office and OpenDocument are zip containers, and are deliberately not here: the shell makes a
// real page-one thumbnail of them, which beats a listing of their guts every time.
static ARCHIVES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    set(".zip .zipx .nupkg .snupkg .jar .war .aar .vsix .apk .whl .crx .epub .xpi .oxt")
});

// .woff/.woff2 are absent on purpose: the text stack cannot load them, so they go to the shell
// like any other file rather than failing inside the font renderer.
static FONTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| set(".ttf .otf .ttc .otc"));

static DOCUMENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    set(
        ".pdf .doc .docx .dot .dotx .xls .xlsx .xlsm .ppt .pptx .pps .ppsx \
         .odt .ods .odp .rtf .pub .vsd .vsdx .one .msg .eml .lnk .url .chm .xps .oxps",
    )
});

static TEXT_FILES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    set(
        ".txt .text .log .nfo .me .readme .lst .diff .patch .rej .orig \
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
         .srt .vtt .sub .ass .reg .m3u .m3u8 .pls .gitignore .gitattributes .editorconfig .lock .sum .mod",
    )
});
