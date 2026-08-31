using System.Windows.Media;
using System.Windows.Media.Imaging;
using BertBrowser.App.Interop;
using BertBrowser.App.Services;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.Preview;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace BertBrowser.App.ViewModels;

/// <summary>One line of a text preview, with the colouring for it.</summary>
public sealed record PreviewLine(int Number, string Text, IReadOnlyList<SyntaxSpan> Spans);

/// <summary>
/// The preview pane for one tab: takes the file list's selection and turns it into something to
/// look at. There is one of these per tab, so nothing here reaches for "the" selection.
/// </summary>
/// <remarks>
/// Four rules shape this class, and each is a thing Explorer's preview pane gets wrong.
///
/// <para><b>No file is held open.</b> Every read opens with <c>FileShare.ReadWrite | Delete</c>,
/// copies into memory and closes before anything is decoded — so previewing a file never blocks
/// renaming, moving or deleting it. The one exception is deliberate and visible: pressing play on
/// a media file hands the path to a <c>MediaElement</c>, which owns it until the selection
/// changes.</para>
///
/// <para><b>Nothing blocks the UI thread.</b> One <see cref="CancellationTokenSource"/> per
/// request, cancel-previous, cancellation swallowed — the same shape the tab's navigation and
/// search debounce already use, and deliberately its own source so a listing refresh cannot cancel
/// a preview or the other way round. Even <c>File.GetAttributes</c> is on the far side of it: on a
/// disconnected network share that call is the one that hangs.</para>
///
/// <para><b>Selection churn is free.</b> A rubber-band drag adds and removes items one at a time;
/// the debounce below means a sweep across two hundred rows costs one preview, not two hundred.</para>
///
/// <para><b>A cloud placeholder is never hydrated.</b> <see cref="PreviewClassifier"/> refuses it
/// before a byte is read.</para>
/// </remarks>
public sealed partial class PreviewPaneViewModel : ObservableObject, IDisposable
{
    /// <summary>Long enough to swallow an arrow-key sweep down a list, short enough that a
    /// deliberate click feels immediate.</summary>
    private const int DebounceMs = 150;

    /// <summary>Pixel width the shell is asked for. Twice a wide pane on a 150% display.</summary>
    private const int ShellPreviewPixels = 1024;

    /// <summary>Images are decoded no wider than this. A 60-megapixel photograph is 240 MB of
    /// bitmap and the pane is a few hundred pixels wide; smaller images are never scaled up.</summary>
    private const int MaxDecodeWidth = 2048;

    /// <summary>Beyond this the text is still shown, uncoloured. Every coloured span becomes an
    /// inline, and tens of thousands of them take seconds to lay out — which would turn arrowing
    /// onto a big file into a freeze.</summary>
    private const int MaxColouredLines = 1_500;

    /// <summary>Shared across every tab: the same photograph previewed in two panes should cost
    /// one shell call, and fifty images is a couple of screens of browsing.</summary>
    private static readonly PreviewImageCache Images = new();

    private readonly AppSettings _settings;
    private CancellationTokenSource _cts = new();

    private FileItemViewModel? _target;
    private int _selectionCount;

    private readonly IArchiveBrowser _archives;
    private readonly IArchiveReader _archiveReader;
    private readonly IArchivePasswords _passwords;

    public PreviewPaneViewModel(
        AppSettings settings, IArchiveBrowser archives, IArchiveReader archiveReader,
        IArchivePasswords passwords)
    {
        _settings = settings;
        _archives = archives;
        _archiveReader = archiveReader;
        _passwords = passwords;
    }

    // --- what the view binds to ---

    [ObservableProperty] private PreviewKind _kind = PreviewKind.None;
    [ObservableProperty] private string _title = "";
    [ObservableProperty] private string _subTitle = "";
    [ObservableProperty] private bool _isLoading;

    /// <summary>Why there is nothing to look at, in words. Never left blank when there is no
    /// preview: a blank pane reads as a bug, and "no preview available" is a real answer.</summary>
    [ObservableProperty] private string? _message;

    [ObservableProperty] private ImageSource? _image;
    [ObservableProperty] private IReadOnlyList<PreviewLine> _lines = [];
    [ObservableProperty] private string _textFooter = "";
    [ObservableProperty] private IReadOnlyList<ArchiveEntry> _archiveEntries = [];
    [ObservableProperty] private string _archiveFooter = "";
    [ObservableProperty] private FontFamily? _fontSpecimen;
    [ObservableProperty] private string _fontFooter = "";
    [ObservableProperty] private IReadOnlyList<MetadataRow> _metadata = [];

    /// <summary>The text as one string, for copying; <see cref="Lines"/> is for rendering.</summary>
    public string TextForCopy { get; private set; } = "";

    /// <summary>Set only once the user presses play. Until then the pane shows a poster frame and
    /// the file is not open — which is also why the harness can photograph a video without ever
    /// starting a media pipeline.</summary>
    [ObservableProperty] private Uri? _mediaSource;

    [ObservableProperty] private bool _canPlayMedia;

    /// <summary>Fit-to-pane, or 1:1 with panning. Double-click toggles it.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(FitLabel))]
    private bool _fitImageToPane = true;

    /// <summary>What the toggle button says it will do next, not what the image is doing now.</summary>
    public string FitLabel => FitImageToPane ? "100%" : "Fit";

    /// <summary>Wrapping and the line-number gutter are mutually exclusive: a wrapped line is not
    /// one row, so a gutter beside it would number the wrong things.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ShowLineNumbers))]
    private bool _wrapText;

    public bool ShowLineNumbers => !WrapText;

    public bool HasMessage => !string.IsNullOrEmpty(Message);
    public bool HasImage => !IsLoading && Image is not null && Kind is PreviewKind.Image or PreviewKind.Document;
    public bool HasText => !IsLoading && Kind == PreviewKind.Text && Lines.Count > 0;
    public bool HasArchive => !IsLoading && Kind == PreviewKind.Archive && ArchiveEntries.Count > 0;
    public bool HasFont => !IsLoading && Kind == PreviewKind.Font && FontSpecimen is not null;
    public bool HasMedia => !IsLoading && Kind == PreviewKind.Media;
    public bool HasMetadata => Metadata.Count > 0;

    /// <summary>Whether the action strip has anything in it, so an empty row of buttons never
    /// takes a line of a pane that is already narrow.</summary>
    public bool HasActions => HasImage || HasText;

    /// <summary>What the harness asserts on, and what a bug report can quote.</summary>
    public string StateName =>
        IsLoading ? "loading"
        : HasImage ? (Kind == PreviewKind.Image ? "image" : "document")
        : HasText ? "text"
        : HasArchive ? "archive"
        : HasFont ? "font"
        : HasMedia ? "media"
        : "none";

    [RelayCommand]
    private void PlayMedia()
    {
        if (_target is { IsDirectory: false } item && Kind == PreviewKind.Media)
            MediaSource = new Uri(item.FullPath);
    }

    [RelayCommand]
    private void ToggleFit() => FitImageToPane = !FitImageToPane;

    [RelayCommand]
    private void ToggleWrap() => WrapText = !WrapText;

    // --- driving it ---

    /// <summary>Called by the view whenever the list's selection changes. Cheap to call often.</summary>
    public void Show(IReadOnlyList<FileItemViewModel> selection)
    {
        _selectionCount = selection.Count;
        var next = selection.Count == 1 ? selection[0] : null;

        // Compared by path, never by instance: the file list swaps its whole collection on a
        // reload and replaces individual rows on a watcher update, so the object identity of "the
        // same file" changes under us constantly. The stamp is in the comparison because an edited
        // file must be re-read.
        var unchanged = next is not null && _target is not null
            && string.Equals(next.FullPath, _target.FullPath, StringComparison.OrdinalIgnoreCase)
            && next.ModifiedUtc == _target.ModifiedUtc;

        _target = next;
        if (unchanged) return;
        Reload();
    }

    /// <summary>Re-runs the current preview, for when the folder changed under it.</summary>
    public void Reload()
    {
        _cts.Cancel();
        _cts = new CancellationTokenSource();
        _ = LoadAsync(_cts.Token);
    }

    public void Dispose()
    {
        _cts.Cancel();
        _cts.Dispose();
    }

    private async Task LoadAsync(CancellationToken ct)
    {
        try
        {
            await Task.Delay(DebounceMs, ct);

            var item = _target;

            // The refusals that need no disk at all are answered here, so selecting a folder or a
            // hundred files never shows a spinner on its way to a one-line message.
            if (item is null)
            {
                Clear(_selectionCount > 1 ? $"{_selectionCount:N0} items selected" : "Select a file to preview");
                return;
            }
            if (item.IsDirectory)
            {
                Describe(item);
                Clear(item.SizeBytes is { } bytes
                    ? $"Folder  ·  {ByteSizeFormatter.Format(bytes)}"
                    : "Folder  ·  size not indexed yet");
                return;
            }

            Describe(item);
            Kind = PreviewKind.None;
            Message = null;
            IsLoading = true;
            Raise();

            var path = item.FullPath;
            var stamp = item.ModifiedUtc;
            var size = item.SizeBytes ?? 0;
            var name = item.Name;
            var budget = Math.Max(4096, _settings.PreviewTextMaxBytes);

            var (plan, payload) = await Task.Run(() =>
            {
                var request = PreviewClassifier.Classify(
                    new PreviewTarget(name, size, AttributesOf(path), IsDirectory: false), budget);
                return (request, request.IsRefused ? null : Build(path, stamp, request, ct));
            }, ct);

            ct.ThrowIfCancellationRequested();

            if (payload is null)
            {
                Kind = PreviewKind.None;
                IsLoading = false;
                Clear(Explain(plan.Refusal));
                Describe(item);
                return;
            }

            Apply(payload);
        }
        catch (OperationCanceledException)
        {
            // A newer selection is already on its way.
        }
    }

    /// <summary>A listing row does not carry the cloud-placeholder bits, so they are read here —
    /// on the background thread, because on a dead network share this is the call that hangs.</summary>
    private static FileAttributes AttributesOf(string path)
    {
        try
        {
            return System.IO.File.GetAttributes(path);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
        {
            return FileAttributes.Normal;
        }
    }

    private void Describe(FileItemViewModel item)
    {
        Title = item.Name;
        var parts = new List<string>(3) { item.TypeName };
        if (item.SizeBytes is { } bytes && !item.IsDirectory) parts.Add(ByteSizeFormatter.Format(bytes));
        if (item.ModifiedUtc != default) parts.Add(item.ModifiedUtc.ToLocalTime().ToString("g"));
        SubTitle = string.Join("  ·  ", parts);
    }

    private string Explain(PreviewRefusal refusal) => refusal switch
    {
        PreviewRefusal.NothingSelected => "Select a file to preview",
        PreviewRefusal.MultipleSelected => $"{_selectionCount:N0} items selected",
        PreviewRefusal.TooLarge => "Too large to preview",
        PreviewRefusal.NotDownloaded =>
            "Stored in the cloud and not downloaded.\nPreviewing it would fetch the whole file, so it is left alone.",
        _ => "No preview available",
    };

    private void Clear(string? message)
    {
        Kind = PreviewKind.None;
        IsLoading = false;
        Image = null;
        Lines = [];
        TextForCopy = "";
        TextFooter = "";
        ArchiveEntries = [];
        ArchiveFooter = "";
        FontSpecimen = null;
        FontFooter = "";
        MediaSource = null;
        CanPlayMedia = false;
        Metadata = [];
        FitImageToPane = true;

        if (_target is null)
        {
            Title = "";
            SubTitle = "";
        }
        Message = message;
        Raise();
    }

    /// <summary>The payload's own kind, not the plan's: a document the shell could not preview but
    /// which reads as text arrives here as text, and the pane has to render what it got rather than
    /// what was expected.</summary>
    private void Apply(Payload payload)
    {
        Kind = payload.Kind;
        Image = payload.Image;
        Lines = payload.Lines;
        TextForCopy = payload.TextForCopy;
        TextFooter = payload.TextFooter;
        ArchiveEntries = payload.Archive;
        ArchiveFooter = payload.ArchiveFooter;
        FontSpecimen = payload.FontSource is { } source ? SafeFontFamily(source) : null;
        FontFooter = payload.FontFooter;
        Metadata = payload.Metadata;
        MediaSource = null;
        CanPlayMedia = payload.Kind == PreviewKind.Media;
        FitImageToPane = true;

        if (payload.ImageWidth > 0)
            SubTitle = $"{payload.ImageWidth} × {payload.ImageHeight}  ·  {SubTitle}";

        IsLoading = false;
        Message = payload.Message;
        Raise();
    }

    /// <summary>A font family is built on the UI thread from a source string, never carried across
    /// one, and a family WPF cannot resolve is nothing rather than a throw.</summary>
    private static FontFamily? SafeFontFamily(string source)
    {
        try
        {
            return new FontFamily(source);
        }
        catch (Exception ex) when (ex is ArgumentException or UriFormatException)
        {
            return null;
        }
    }

    private void Raise()
    {
        OnPropertyChanged(nameof(HasImage));
        OnPropertyChanged(nameof(HasText));
        OnPropertyChanged(nameof(HasArchive));
        OnPropertyChanged(nameof(HasFont));
        OnPropertyChanged(nameof(HasMedia));
        OnPropertyChanged(nameof(HasMessage));
        OnPropertyChanged(nameof(HasMetadata));
        OnPropertyChanged(nameof(HasActions));
        OnPropertyChanged(nameof(StateName));
    }

    // --- the background half ---

    private sealed record Payload
    {
        /// <summary>What this actually turned out to be, which is not always what the classifier
        /// planned: a document the shell cannot preview but which reads as text comes back as
        /// <see cref="PreviewKind.Text"/>. <see cref="PreviewKind.None"/> means the payload is
        /// nothing but its <see cref="Message"/>.</summary>
        public PreviewKind Kind { get; init; }

        public ImageSource? Image { get; init; }
        public int ImageWidth { get; init; }
        public int ImageHeight { get; init; }
        public IReadOnlyList<PreviewLine> Lines { get; init; } = [];
        public string TextForCopy { get; init; } = "";
        public string TextFooter { get; init; } = "";
        public IReadOnlyList<ArchiveEntry> Archive { get; init; } = [];
        public string ArchiveFooter { get; init; } = "";
        public string? FontSource { get; init; }
        public string FontFooter { get; init; } = "";
        public IReadOnlyList<MetadataRow> Metadata { get; init; } = [];
        public string? Message { get; init; }
    }

    private Payload Build(string path, DateTime stamp, PreviewRequest plan, CancellationToken ct)
    {
        try
        {
            // Inside a container the differences are all subtractions, and each is a thing that
            // needs a real path rather than bytes. The shell cannot see a path that does not
            // exist, so asking it would cost a round trip per selection to be told nothing; a
            // MediaElement and a GlyphTypeface both need a Uri, so those two say what to do instead
            // of failing obscurely. Everything else is unchanged, because the classifier takes a
            // name and every reader below takes a Stream.
            var inArchive = _archives.Resolve(path) is { IsRoot: false };

            var payload = plan.Kind switch
            {
                PreviewKind.Image => BuildImage(path, stamp),
                PreviewKind.Text => BuildText(path, plan, ct),
                PreviewKind.Archive => BuildArchive(path),
                PreviewKind.Font when inArchive =>
                    new Payload { Message = "Extract this font to preview it." },
                PreviewKind.Font => BuildFont(path),
                PreviewKind.Media when inArchive =>
                    new Payload { Message = "Extract this file to play it." },
                // Media shows a poster frame if the shell has one, and says nothing if it does not
                // — there is still a transport to press, so "no preview available" would be wrong.
                PreviewKind.Media => BuildShell(path, stamp) with { Kind = PreviewKind.Media, Message = null },
                // The shell half of BuildDocument is skipped, so the order inverts to bytes-only.
                _ when inArchive => BuildText(path, plan, ct, guessing: true),
                _ => BuildDocument(path, stamp, plan, ct),
            };

            ct.ThrowIfCancellationRequested();

            // Shell properties come from the shell, which has never heard of a path inside a
            // container. Skipped rather than attempted: the strip would be empty either way, and
            // this way it costs nothing per selection.
            return payload with
            {
                Metadata = inArchive ? [] : ReadMetadata(path, plan.Kind),
            };
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (ArchiveLockedException ex)
        {
            // Before the general arm below, which is its base type: this one has a reason worth
            // repeating and something the user can act on.
            return new Payload { Message = ex.Message };
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or NotSupportedException or ArgumentException)
        {
            return new Payload { Message = "This file could not be read." };
        }
    }

    private static IReadOnlyList<MetadataRow> ReadMetadata(string path, PreviewKind kind)
    {
        var rows = ShellProperties.Read(path)
            .Select(p => new ShellPropertyRow(p.Canonical, p.Name, p.Value))
            .ToList();
        return PreviewMetadata.Select(kind, rows);
    }

    /// <summary>Opens sharing everything, copies into memory, closes. The handle is gone before a
    /// single pixel is decoded — which is the whole difference from Explorer's pane.</summary>
    /// <summary>
    /// The one place a preview gets its bytes, from a real file or from inside a container.
    /// </summary>
    /// <remarks>
    /// <para>
    /// Routing here rather than at each builder is what let the whole pane learn about archives
    /// without any of it learning what an archive is: the classifier already takes a
    /// <em>name</em>, and every reader below already takes a <see cref="Stream"/>.
    /// </para>
    /// <para>
    /// <b>Bounding the read bounds the decompression</b>, which is the one thing a zip bomb cannot
    /// get around — pulling a megabyte out of a stream that would have produced ten gigabytes costs
    /// a megabyte. So the budgets that were already here are the right shape, and no expansion-ratio
    /// check is needed.
    /// </para>
    /// </remarks>
    private Stream OpenBounded(string path, long limit)
    {
        if (_archives.Resolve(path) is { IsRoot: false } entry)
        {
            var index = _archives.ReadArchive(entry.ArchiveFile);
            var password = _passwords.For(entry.ArchiveFile);

            var bytes = _archiveReader.ReadEntry(
                entry.ArchiveFile, entry.EntryPath, limit, password);

            if (bytes is null)
            {
                // An encrypted zip lists in full and only refuses its contents, so this is the
                // ordinary way a preview fails in one — and "could not be read" would send the user
                // looking for a corrupt file rather than at the Unlock button above the list.
                throw index.Find(entry.EntryPath) is { IsEncrypted: true }
                    ? new ArchiveLockedException(
                        entry.ArchiveFile, "This file is encrypted. Unlock the archive to preview it.")
                    : new IOException("That entry could not be read from the archive.");
            }

            return new MemoryStream(bytes);
        }

        return new FileStream(
            path, FileMode.Open, FileAccess.Read,
            FileShare.ReadWrite | FileShare.Delete,
            bufferSize: 64 * 1024, FileOptions.SequentialScan);
    }

    private MemoryStream ReadFully(string path, long limit)
    {
        using var file = OpenBounded(path, limit);

        var buffer = new MemoryStream(capacity: 64 * 1024);
        var remaining = limit;
        var chunk = new byte[64 * 1024];
        while (remaining > 0)
        {
            var read = file.Read(chunk, 0, (int)Math.Min(chunk.Length, remaining));
            if (read == 0) break;
            buffer.Write(chunk, 0, read);
            remaining -= read;
        }
        buffer.Position = 0;
        return buffer;
    }

    private Payload BuildImage(string path, DateTime stamp)
    {
        using var bytes = ReadFully(path, PreviewClassifier.MaxImageBytes);
        try
        {
            // The header is read first, so a small image is never scaled up to the decode cap and
            // a huge one is never decoded at full size.
            bytes.Position = 0;
            var probe = BitmapDecoder.Create(bytes, BitmapCreateOptions.DelayCreation, BitmapCacheOption.None);
            var frame = probe.Frames[0];
            int width = frame.PixelWidth, height = frame.PixelHeight;

            bytes.Position = 0;
            var image = new BitmapImage();
            image.BeginInit();
            image.StreamSource = bytes;
            image.CacheOption = BitmapCacheOption.OnLoad; // decodes now, so the stream can go
            if (width > MaxDecodeWidth) image.DecodePixelWidth = MaxDecodeWidth;
            image.EndInit();
            image.Freeze();

            return new Payload { Kind = PreviewKind.Image, Image = image, ImageWidth = width, ImageHeight = height };
        }
        catch (Exception ex) when (ex is NotSupportedException or FileFormatException or ArgumentException or OverflowException or InvalidOperationException)
        {
            // No WIC codec for this format — HEIC and camera raw on a machine without the
            // extensions, most often. The shell may still have a handler for it.
            return BuildShell(path, stamp) with { Kind = PreviewKind.Image };
        }
    }

    /// <param name="guessing">True when nothing about the file <em>said</em> it was text and this
    /// is the document fallback having a look. The bar for showing it is higher, and so is the bar
    /// for the message: "binary file" is a fact when a .txt turns out not to be text, and a guess
    /// dressed as one when we only opened it on spec.</param>
    private Payload BuildText(string path, PreviewRequest plan, CancellationToken ct, bool guessing = false)
    {
        using var file = OpenBounded(path, plan.ByteBudget);

        var preview = TextPreviewReader.Read(file, plan.ByteBudget);
        ct.ThrowIfCancellationRequested();

        if (guessing)
        {
            if (!TextPreviewReader.IsConvincingText(preview))
                return new Payload { Message = "No preview available" };
        }
        else if (preview.LooksBinary)
        {
            return new Payload { Message = "Binary file — nothing to show as text." };
        }

        var coloured = preview.LineCount <= MaxColouredLines;
        IReadOnlyList<SyntaxSpan> spans = coloured
            ? SyntaxTokenizer.Tokenize(preview.Text, plan.Language)
            : [];

        var footer = Join(
            preview.EncodingName,
            preview.LineEnding,
            $"{preview.LineCount:N0} lines",
            preview.Truncated ? "truncated" : "",
            coloured ? "" : "colouring off (large file)");

        return new Payload
        {
            Kind = PreviewKind.Text,
            Lines = SplitLines(preview.Text, spans),
            TextForCopy = preview.Text,
            TextFooter = footer,
        };
    }

    /// <summary>Cuts the text into lines and distributes the tokenizer's spans across them. The
    /// spans cover the text exactly once and in order, so this is a single pass with no
    /// lookahead — a span straddling a newline is simply visited twice.</summary>
    private static IReadOnlyList<PreviewLine> SplitLines(string text, IReadOnlyList<SyntaxSpan> spans)
    {
        if (text.Length == 0) return [new PreviewLine(1, "", [])];

        var lines = new List<PreviewLine>();
        var spanIndex = 0;
        var lineStart = 0;
        var number = 1;

        while (lineStart < text.Length)
        {
            var newline = text.IndexOf('\n', lineStart);
            var lineEnd = newline < 0 ? text.Length : newline;

            var lineSpans = new List<SyntaxSpan>();
            while (spanIndex < spans.Count && spans[spanIndex].Start < lineEnd)
            {
                var span = spans[spanIndex];
                var from = Math.Max(span.Start, lineStart);
                var to = Math.Min(span.Start + span.Length, lineEnd);
                if (to > from) lineSpans.Add(new SyntaxSpan(from - lineStart, to - from, span.Class));

                if (span.Start + span.Length > lineEnd) break; // continues on the next line
                spanIndex++;
            }

            lines.Add(new PreviewLine(number++, text[lineStart..lineEnd], lineSpans));
            if (newline < 0) break;
            lineStart = newline + 1;
        }
        return lines;
    }

    /// <remarks>
    /// Still <c>ArchiveListing</c> rather than the browse index, and the two entry caps stay
    /// different numbers on purpose: this one runs on arrow keys and must stay cheap, where a
    /// listing you navigated to was asked for. Reading through <see cref="OpenBounded"/> means a
    /// zip nested inside another one still gets <em>listed</em> here even though it cannot be
    /// entered.
    /// </remarks>
    private Payload BuildArchive(string path)
    {
        using var file = OpenBounded(path, PreviewClassifier.MaxArchiveBytes);

        var contents = ArchiveListing.Read(file);
        if (contents.Error is { } error) return new Payload { Message = error };

        var footer = Join(
            $"{contents.TotalCount:N0} entries",
            ByteSizeFormatter.Format(contents.TotalBytes) + " uncompressed",
            contents.CompressionRatio > 0 ? $"{contents.CompressionRatio:P0} saved" : "",
            contents.Truncated ? $"showing the first {contents.Entries.Count:N0}" : "");

        return new Payload { Kind = PreviewKind.Archive, Archive = contents.Entries, ArchiveFooter = footer };
    }

    private Payload BuildFont(string path)
    {
        try
        {
            var typeface = new GlyphTypeface(new Uri(path));
            var family =
                typeface.Win32FamilyNames.Values.FirstOrDefault()
                ?? typeface.FamilyNames.Values.FirstOrDefault();
            if (string.IsNullOrEmpty(family))
                return new Payload { Message = "No font family name in this file." };

            // WPF resolves a private font as "<folder uri>#<family name>"; only that string
            // crosses back to the UI thread, where the FontFamily itself is built.
            var folder = Path.GetDirectoryName(path);
            if (string.IsNullOrEmpty(folder))
                return new Payload { Message = "No font faces could be read from this file." };

            var source = new Uri(folder.EndsWith('\\') ? folder : folder + "\\").AbsoluteUri + "#" + family;
            var footer = Join(
                family,
                typeface.FaceNames.Values.FirstOrDefault() ?? "",
                $"{typeface.CharacterToGlyphMap.Count:N0} glyphs",
                typeface.VersionStrings.Values.FirstOrDefault() ?? "");

            return new Payload { Kind = PreviewKind.Font, FontSource = source, FontFooter = footer };
        }
        catch (Exception ex) when (ex is FileFormatException or NotSupportedException or ArgumentException or UriFormatException)
        {
            return new Payload { Message = "This font could not be read." };
        }
    }

    private Payload BuildShell(string path, DateTime stamp)
    {
        var key = PreviewImageCache.KeyFor(path, ShellPreviewPixels, stamp);
        var image = Images.GetOrAdd(key, () => ShellThumbnails.GetPreview(path, ShellPreviewPixels));

        return image is null
            ? new Payload { Message = "No preview available" }
            : new Payload { Kind = PreviewKind.Document, Image = image };
    }

    /// <summary>
    /// A document: whatever the shell can produce, and failing that, a look at the actual bytes.
    /// </summary>
    /// <remarks>
    /// The fallback is the point. An extension table cannot be the whole answer — its tail is
    /// endless, and every entry missing from it is a file the pane refuses to show for no reason
    /// the user can see. <c>choco.exe.manifest</c> is plainly XML; <c>.ignore</c> beside it is
    /// plainly a list of names. Explorer gives up at exactly this point, and it is the most common
    /// way its preview pane is useless.
    ///
    /// The shell goes first because when it does have a handler its answer is better: a .docx is a
    /// zip and reads as gibberish, but the shell has a page-one thumbnail of it. Only when the
    /// shell declines outright is the file read — so this costs nothing for the formats that
    /// already worked.
    /// </remarks>
    private Payload BuildDocument(string path, DateTime stamp, PreviewRequest plan, CancellationToken ct)
    {
        var shell = BuildShell(path, stamp);

        // A budget of zero is the classifier saying "don't guess at this one" — an image too big to
        // decode is still an image.
        if (shell.Image is not null || plan.ByteBudget <= 0) return shell;

        var text = BuildText(path, plan, ct, guessing: true);
        return text.Lines.Count > 0 ? text : shell;
    }

    private static string Join(params string[] parts) =>
        string.Join("  ·  ", parts.Where(p => !string.IsNullOrWhiteSpace(p)));
}
