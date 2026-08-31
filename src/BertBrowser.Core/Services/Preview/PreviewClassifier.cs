namespace BertBrowser.Core.Services.Preview;

/// <summary>
/// Decides what the preview pane will do with a selection, touching nothing. This is the planner
/// of the preview feature: every rule that can be settled without opening the file lives here, so
/// the rules are unit-tested rather than inferred from what the pane happens to render.
/// </summary>
/// <remarks>
/// The two decisions worth understanding are the ones that differ from Explorer. A cloud
/// placeholder is <see cref="PreviewRefusal.NotDownloaded"/> rather than a silent multi-gigabyte
/// download; and an unrecognised extension becomes <see cref="PreviewKind.Document"/> — an honest
/// attempt through the shell — rather than an immediate refusal, because the classifier cannot know
/// which preview handlers this machine has.
/// </remarks>
public static class PreviewClassifier
{
    /// <summary>How much of a text file is read by default. Enough for any source file; small
    /// enough that a stray multi-gigabyte log costs a millisecond rather than the session.</summary>
    public const long DefaultTextBudget = 1L << 20; // 1 MB

    /// <summary>Above this an image is not decoded here; the shell is asked for a preview
    /// instead, which it can produce without materialising the whole bitmap.</summary>
    public const long MaxImageBytes = 64L << 20; // 64 MB

    /// <summary>Above this an archive's directory is not read. Nothing streams a listing out of a
    /// 2 GB container fast enough to be worth doing while someone arrows down a list.</summary>
    public const long MaxArchiveBytes = 2L << 30; // 2 GB

    /// <summary>Above this a font file is refused: a real one is a few hundred kilobytes, so
    /// anything past this is not a font we want to hand to the text stack.</summary>
    public const long MaxFontBytes = 32L << 20; // 32 MB

    /// <summary>FILE_ATTRIBUTE_RECALL_ON_OPEN. .NET's <see cref="FileAttributes"/> stops short of
    /// the two cloud-placeholder bits, so they are named here rather than written as magic numbers
    /// at the point of use.</summary>
    public const FileAttributes RecallOnOpen = (FileAttributes)0x00040000;

    /// <summary>FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS — what OneDrive marks a "cloud-only" file
    /// with once its content has been freed up.</summary>
    public const FileAttributes RecallOnDataAccess = (FileAttributes)0x00400000;

    /// <summary>Cloud-placeholder attributes. Reading a file carrying any of these makes the
    /// provider fetch its content, which is never something a preview may decide on its own.</summary>
    private const FileAttributes Placeholder =
        FileAttributes.Offline | RecallOnOpen | RecallOnDataAccess;

    public static PreviewRequest Classify(
        IReadOnlyList<PreviewTarget> selection,
        long textBudget = DefaultTextBudget,
        PreviewMode mode = PreviewMode.Auto) =>
        selection.Count switch
        {
            0 => Refuse(PreviewRefusal.NothingSelected),
            1 => Classify(selection[0], textBudget, mode),
            _ => Refuse(PreviewRefusal.MultipleSelected),
        };

    public static PreviewRequest Classify(
        PreviewTarget target,
        long textBudget = DefaultTextBudget,
        PreviewMode mode = PreviewMode.Auto)
    {
        if (target.IsDirectory)
            return Refuse(PreviewRefusal.Folder);

        if ((target.Attributes & Placeholder) != 0)
            return Refuse(PreviewRefusal.NotDownloaded);

        // The override comes after the two refusals above and never before them. Forcing a mode
        // says how to render bytes we were already willing to read; it is not permission to read
        // bytes we refused — and a placeholder refused here is a download that does not happen.
        // Neither forced mode carries a language: raw means uncoloured, which is the whole
        // difference between it and the ordinary text path over a .cs.
        if (mode is PreviewMode.Hex or PreviewMode.Text)
        {
            return new PreviewRequest(
                mode == PreviewMode.Hex ? PreviewKind.Hex : PreviewKind.Text,
                PreviewRefusal.None,
                Math.Min(target.SizeBytes, Math.Max(0, textBudget)),
                SyntaxLanguage.None,
                mode);
        }

        var kind = KindFor(target.Name);
        return kind switch
        {
            // Too big to decode ourselves is not too big to preview: the shell's handler can
            // produce a thumbnail of a 500 MB TIFF without us reading one byte of it.
            // A zero budget, unlike an ordinary document's: this is still an image, and reading a
            // gigantic TIFF as text if the shell declines would be nonsense.
            PreviewKind.Image when target.SizeBytes > MaxImageBytes =>
                new PreviewRequest(PreviewKind.Document, PreviewRefusal.None, 0, SyntaxLanguage.None),

            PreviewKind.Archive when target.SizeBytes > MaxArchiveBytes => Refuse(PreviewRefusal.TooLarge),
            PreviewKind.Font when target.SizeBytes > MaxFontBytes => Refuse(PreviewRefusal.TooLarge),

            // Text is never refused for size — it is truncated, and the pane says so.
            PreviewKind.Text => new PreviewRequest(
                PreviewKind.Text, PreviewRefusal.None,
                Math.Min(target.SizeBytes, Math.Max(0, textBudget)),
                SyntaxTokenizer.LanguageFor(target.Name)),

            PreviewKind.Image => new PreviewRequest(kind, PreviewRefusal.None, target.SizeBytes, SyntaxLanguage.None),
            PreviewKind.Archive => new PreviewRequest(kind, PreviewRefusal.None, target.SizeBytes, SyntaxLanguage.None),
            PreviewKind.Font => new PreviewRequest(kind, PreviewRefusal.None, target.SizeBytes, SyntaxLanguage.None),

            // A document carries a text budget even though the shell is asked first: it is what the
            // text fallback may read when the shell turns out to have no preview for it. That
            // fallback is the answer to an extension table's endless tail — a `.manifest`, a config
            // file with a name nobody standardised, an extensionless script — and it is exactly
            // where Explorer's pane gives up.
            PreviewKind.Document => new PreviewRequest(
                kind, PreviewRefusal.None,
                Math.Min(target.SizeBytes, Math.Max(0, textBudget)),
                SyntaxTokenizer.LanguageFor(target.Name)),

            // Media is the shell's to stream, and there is nothing to read as text.
            _ => new PreviewRequest(kind, PreviewRefusal.None, 0, SyntaxLanguage.None),
        };
    }

    private static PreviewRequest Refuse(PreviewRefusal refusal) =>
        new(PreviewKind.None, refusal, 0, SyntaxLanguage.None);

    /// <summary>The extension table. Anything unlisted is <see cref="PreviewKind.Document"/> —
    /// the shell gets a try, and only its refusal produces "no preview available".</summary>
    public static PreviewKind KindFor(string name)
    {
        var extension = Path.GetExtension(name);

        // An extensionless file is a source file far more often than it is anything else
        // (Dockerfile, Makefile, LICENSE, hosts), and reading one as text is harmless where
        // reading it as a document is useless.
        if (extension.Length == 0)
            return PreviewKind.Text;

        if (Images.Contains(extension)) return PreviewKind.Image;
        if (Media.Contains(extension)) return PreviewKind.Media;
        if (Archives.Contains(extension)) return PreviewKind.Archive;
        if (Fonts.Contains(extension)) return PreviewKind.Font;
        if (Documents.Contains(extension)) return PreviewKind.Document;
        if (TextFiles.Contains(extension)) return PreviewKind.Text;

        // A leading-dot file (".gitignore") has the whole name as its "extension".
        if (SyntaxTokenizer.LanguageFor(name) != SyntaxLanguage.None) return PreviewKind.Text;

        return PreviewKind.Document;
    }

    private static HashSet<string> Set(string extensions) =>
        new(extensions.Split(' ', StringSplitOptions.RemoveEmptyEntries), StringComparer.OrdinalIgnoreCase);

    // WIC decodes the first group directly; the rest need a codec or a shell handler, and fall
    // back to the shell preview when one is missing. Both are Image either way — the pane tries
    // the decoder first and the shell second, so the table does not have to know what is installed.
    private static readonly HashSet<string> Images = Set(
        ".png .jpg .jpeg .jfif .jpe .bmp .gif .tif .tiff .ico .wdp .jxr " +
        ".webp .heic .heif .avif .psd .tga .exr " +
        ".raw .cr2 .cr3 .nef .arw .dng .orf .rw2 .raf .pef .sr2 .srw .3fr .erf .kdc .mrw");

    private static readonly HashSet<string> Media = Set(
        ".mp3 .wav .flac .m4a .m4b .aac .ogg .oga .opus .wma .aif .aiff .ape .mid .midi " +
        ".mp4 .mov .avi .mkv .wmv .webm .m4v .flv .mpg .mpeg .3gp .3g2 .m2ts .mts .vob .ogv .asf .rm");

    // Office and OpenDocument are zip containers, and are deliberately not here: the shell makes a
    // real page-one thumbnail of them, which beats a listing of their guts every time.
    private static readonly HashSet<string> Archives = Set(
        ".zip .zipx .nupkg .snupkg .jar .war .aar .vsix .apk .whl .crx .epub .xpi .oxt");

    // .woff/.woff2 are absent on purpose: the text stack cannot load them, so they go to the
    // shell like any other file rather than failing inside the font renderer.
    private static readonly HashSet<string> Fonts = Set(".ttf .otf .ttc .otc");

    private static readonly HashSet<string> Documents = Set(
        ".pdf .doc .docx .dot .dotx .xls .xlsx .xlsm .ppt .pptx .pps .ppsx " +
        ".odt .ods .odp .rtf .pub .vsd .vsdx .one .msg .eml .lnk .url .chm .xps .oxps");

    private static readonly HashSet<string> TextFiles = Set(
        ".txt .text .log .nfo .me .readme .lst .diff .patch .rej .orig " +
        ".md .markdown .mdx .rst .adoc .asciidoc .tex .bib .org " +
        ".json .jsonc .json5 .webmanifest .ipynb .yml .yaml .toml .ini .cfg .conf .properties .env " +
        ".xml .xaml .html .htm .xhtml .svg .csproj .fsproj .vbproj .sln .slnx .props .targets " +
        ".config .resx .nuspec .plist .csv .tsv .psv " +
        // The XML family the extension table used to miss. Listed rather than left to the content
        // sniff purely so they arrive coloured and without a wasted shell round-trip; the sniff is
        // still what catches everything not named here.
        ".manifest .appxmanifest .xsd .xsl .xslt .dtd .rss .atom .opml .sitemap .axaml " +
        ".vcxproj .shproj .pubxml .ruleset .runsettings .wxs .wxi .storyboard " +
        ".cshtml .razor .vbhtml .aspx .ascx .ashx .asmx .master .jsp " +
        ".erb .hbs .mustache .twig .pug .ejs .liquid " +
        ".gitmodules .gitconfig .npmignore .prettierrc .eslintrc .babelrc .stylelintrc " +
        ".clang-format .htaccess .ignore .po .pot .strings .desktop .service .cue .ics .vcf " +
        ".f90 .for .pas .cr .rkt .scm .lisp .el .ahk .au3 .bas .vbs .wsf .applescript " +
        ".ninja .bzl .gn .gni .pro .in .ac .am " +
        ".css .scss .less .sass .styl " +
        ".js .mjs .cjs .jsx .ts .tsx .vue .svelte .astro " +
        ".cs .csx .fs .fsx .vb .c .h .cpp .hpp .cc .hh .cxx .m .mm .java .kt .kts .swift .go .rs " +
        ".php .rb .py .pyw .pyi .pl .pm .lua .r .jl .scala .dart .groovy .gradle .clj .cljs .ex .exs " +
        ".erl .hs .ml .mli .nim .zig .v .sv .vhd .asm .s .proto .graphql .gql .thrift " +
        ".sql .ps1 .psm1 .psd1 .sh .bash .zsh .fish .bat .cmd .awk .sed .vim " +
        ".tf .tfvars .hcl .bicep .nix .cmake .mk .make .dockerfile .bbs " +
        ".srt .vtt .sub .ass .reg .m3u .m3u8 .pls .gitignore .gitattributes .editorconfig .lock .sum .mod");
}
