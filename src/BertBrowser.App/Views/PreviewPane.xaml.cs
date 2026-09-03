using System.ComponentModel;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Documents;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services.Preview;
using BertBrowser.Core.Theming;

namespace BertBrowser.App.Views;

/// <summary>
/// The view half of <see cref="PreviewPaneViewModel"/>. Everything here is presentation the view
/// model cannot own: a flow document, a scroll offset, a tiling brush and a media transport are
/// all element state.
/// </summary>
public partial class PreviewPane : UserControl
{
    /// <summary>Fixed, and shared by the editor and the gutter — a gutter can only line up with
    /// the text beside it if every line is exactly one row tall.</summary>
    private const double LineHeight = 16;

    private static readonly FontFamily Monospace = new("Cascadia Mono, Consolas, Courier New, monospace");

    private readonly DispatcherTimer _mediaTick = new() { Interval = TimeSpan.FromMilliseconds(250) };
    private bool _seeking;
    private bool _playing;

    private PreviewPaneViewModel? _model;
    private SolidColorBrush? _checkerLight;
    private SolidColorBrush? _checkerDark;

    public PreviewPane()
    {
        InitializeComponent();
        Gutter.LineHeight = LineHeight;
        Gutter.FontFamily = Monospace;
        Gutter.FontSize = 12;

        _mediaTick.Tick += MediaTick;
        DataContextChanged += OnDataContextChanged;
        Loaded += OnLoaded;
        Unloaded += OnUnloaded;
    }

    private void OnLoaded(object sender, RoutedEventArgs e)
    {
        HookCheckerboard();
        HookGutterScrolling();
    }

    /// <summary>Raised when the fit-width button is pressed. The pane's own column belongs to
    /// <c>DirectoryTabView</c> — <c>ColumnDefinition.Width</c> is not bindable, the same reason
    /// its width is already assigned from that view's code-behind — so this only asks; it does
    /// not resize anything itself.</summary>
    public event EventHandler? FitWidthRequested;

    private void FitWidth_Click(object sender, RoutedEventArgs e) => FitWidthRequested?.Invoke(this, EventArgs.Empty);

    /// <summary>
    /// How wide the pane would need to be to show what it currently holds with no horizontal
    /// scrollbar. Only text, a hex dump and an image have a natural width beyond "whatever it is
    /// now" — everything else (archive listing, font specimen, media transport) already lays out
    /// to fit, so their own current width is the honest answer.
    /// </summary>
    public double MeasureDesiredWidth()
    {
        if (_model is null) return ActualWidth;
        return _model.Kind switch
        {
            PreviewKind.Text or PreviewKind.Hex => MeasureTextWidth(),
            PreviewKind.Image or PreviewKind.Document => MeasureImageWidth(),
            _ => ActualWidth,
        };
    }

    /// <summary>Widest rendered line, plus the gutter and the padding the document itself
    /// carries. Measures every line rather than sampling: the pane already builds one paragraph
    /// per line in <see cref="Rebuild"/>, so this is no bigger an ask than the render it follows.</summary>
    private double MeasureTextWidth()
    {
        var lines = _model?.Lines ?? [];
        if (lines.Count == 0) return ActualWidth;

        var typeface = new Typeface(Monospace, FontStyles.Normal, FontWeights.Normal, FontStretches.Normal);
        var dpi = VisualTreeHelper.GetDpi(this).PixelsPerDip;
        var widest = 0.0;
        foreach (var line in lines)
        {
            if (line.Text.Length == 0) continue;
            var formatted = new FormattedText(
                line.Text, System.Globalization.CultureInfo.CurrentCulture, FlowDirection.LeftToRight,
                typeface, 12, Brushes.Black, dpi);
            if (formatted.WidthIncludingTrailingWhitespace > widest) widest = formatted.WidthIncludingTrailingWhitespace;
        }

        var gutterWidth = _model?.ShowLineNumbers == true ? Gutter.ActualWidth + 1 : 0;
        return widest + gutterWidth + 12 /* PagePadding */ + SystemParameters.VerticalScrollBarWidth + 8;
    }

    /// <summary>The image's own size — <see cref="BitmapSource.Width"/> is already in
    /// device-independent pixels, so no DPI conversion is needed.</summary>
    private double MeasureImageWidth() =>
        PreviewImage.Source is BitmapSource bitmap ? bitmap.Width + 16 : ActualWidth;

    private void OnUnloaded(object sender, RoutedEventArgs e) => _mediaTick.Stop();

    private void OnDataContextChanged(object sender, DependencyPropertyChangedEventArgs e)
    {
        if (_model is not null) _model.PropertyChanged -= OnModelChanged;
        _model = DataContext as PreviewPaneViewModel;
        if (_model is not null) _model.PropertyChanged += OnModelChanged;
        Rebuild();
    }

    /// <summary>Gives the subscriptions back. A tab is closable, so its view has to let go.</summary>
    public void Detach()
    {
        if (_model is not null) _model.PropertyChanged -= OnModelChanged;
        _model = null;
        _mediaTick.Stop();
        MediaView.Close();
        UnhookCheckerboard();
    }

    private void OnModelChanged(object? sender, PropertyChangedEventArgs e)
    {
        switch (e.PropertyName)
        {
            case nameof(PreviewPaneViewModel.Lines):
            case nameof(PreviewPaneViewModel.WrapText):
                Rebuild();
                break;
            case nameof(PreviewPaneViewModel.FitImageToPane):
                ApplyImageFit();
                break;
            case nameof(PreviewPaneViewModel.MediaSource):
                StartOrStopMedia();
                break;
        }
    }

    // --- text ---

    /// <summary>
    /// Builds the flow document and the gutter from the view model's lines.
    /// </summary>
    /// <remarks>
    /// A <see cref="RichTextBox"/> rather than a stack of <c>TextBlock</c>s because the text has to
    /// be selectable, which is the thing Explorer's preview pane cannot do. The cost is that every
    /// coloured run is an inline, which is why <c>PreviewPaneViewModel</c> stops colouring past a
    /// line count and hands us plain lines instead — the document still builds, just with one run
    /// per line.
    /// </remarks>
    private void Rebuild()
    {
        var lines = _model?.Lines ?? [];
        if (lines.Count == 0)
        {
            TextView.Document = new FlowDocument();
            Gutter.Text = "";
            return;
        }

        var wrap = _model?.WrapText == true;
        var document = new FlowDocument
        {
            FontFamily = Monospace,
            FontSize = 12,
            LineHeight = LineHeight,
            LineStackingStrategy = LineStackingStrategy.BlockLineHeight,
            PagePadding = new Thickness(6, 4, 6, 4),
            // A wide page is how a RichTextBox is told not to wrap; the horizontal scrollbar then
            // does the rest.
            PageWidth = wrap ? double.NaN : 4000,
        };

        foreach (var line in lines)
        {
            var paragraph = new Paragraph { Margin = default };
            if (line.Spans.Count == 0)
            {
                paragraph.Inlines.Add(new Run(line.Text));
            }
            else
            {
                foreach (var span in line.Spans)
                {
                    var run = new Run(line.Text.Substring(span.Start, span.Length));
                    if (BrushFor(span.Class) is { } brush) run.Foreground = brush;
                    paragraph.Inlines.Add(run);
                }
            }
            document.Blocks.Add(paragraph);
        }

        TextView.Document = document;
        TextView.HorizontalScrollBarVisibility = wrap ? ScrollBarVisibility.Disabled : ScrollBarVisibility.Auto;
        Gutter.Text = string.Join('\n', lines.Select(l => l.Number.ToString()));
        GutterOffset.Y = 0;
        HookGutterScrolling();
    }

    /// <summary>Null for <see cref="SyntaxClass.Text"/>, so ordinary text inherits the pane's
    /// foreground and follows a theme change without a token of its own.</summary>
    private Brush? BrushFor(SyntaxClass syntax)
    {
        var key = syntax switch
        {
            SyntaxClass.Keyword => ThemeToken.SyntaxKeyword,
            SyntaxClass.String => ThemeToken.SyntaxString,
            SyntaxClass.Comment => ThemeToken.SyntaxComment,
            SyntaxClass.Number => ThemeToken.SyntaxNumber,
            SyntaxClass.Punctuation => ThemeToken.SyntaxPunctuation,
            _ => null,
        };
        return key is null ? null : TryFindResource(key) as Brush;
    }

    /// <summary>Ties the gutter to the editor's scroll offset. The editor's <see
    /// cref="ScrollViewer"/> only exists once its template has been applied, and a rebuilt document
    /// does not replace it — but the first call happens before the template is there, so this is
    /// idempotent and called again from <see cref="Rebuild"/>.</summary>
    private void HookGutterScrolling()
    {
        if (_gutterScroller is not null) return;
        _gutterScroller = VisualTreeUtil.FindDescendant<ScrollViewer>(TextView);
        if (_gutterScroller is null) return;
        _gutterScroller.ScrollChanged += (_, e) => GutterOffset.Y = -e.VerticalOffset;
    }

    private ScrollViewer? _gutterScroller;

    private void CopyText_Click(object sender, RoutedEventArgs e)
    {
        var text = _model?.TextForCopy;
        if (string.IsNullOrEmpty(text)) return;
        try
        {
            Clipboard.SetText(text);
        }
        catch (System.Runtime.InteropServices.COMException)
        {
            // Another process had the clipboard open. Nothing to recover, and nothing worth a
            // dialog over a copy.
        }
    }

    // --- image ---

    private void Image_MouseDown(object sender, MouseButtonEventArgs e)
    {
        if (e.ClickCount == 2) _model?.ToggleFitCommand.Execute(null);
    }

    /// <summary>Fit uses <c>StretchDirection=DownOnly</c> so a small image is shown at its own size
    /// rather than blown up; 100% clears the stretch entirely and lets the scroll viewer pan.</summary>
    private void ApplyImageFit()
    {
        var fit = _model?.FitImageToPane != false;
        PreviewImage.Stretch = fit ? Stretch.Uniform : Stretch.None;
        PreviewImage.StretchDirection = fit ? StretchDirection.DownOnly : StretchDirection.Both;
        ImageScroller.ScrollToHome();
    }

    /// <summary>
    /// The chequerboard behind a transparent image, rebuilt whenever the theme recolours.
    /// </summary>
    /// <remarks>
    /// A tiling brush caches its realisation, and a <c>SolidColorBrush</c> inside one changing
    /// colour does not invalidate that cache — the same trap the harness's capture code hit. The
    /// token brushes are live objects whose <c>Color</c> is bound, so their <c>Changed</c> event is
    /// the signal to build a new brush rather than expect the old one to repaint.
    /// </remarks>
    private void HookCheckerboard()
    {
        if (_checkerLight is not null) return;
        _checkerLight = TryFindResource(ThemeToken.PreviewCheckerLight) as SolidColorBrush;
        _checkerDark = TryFindResource(ThemeToken.PreviewCheckerDark) as SolidColorBrush;
        if (_checkerLight is null || _checkerDark is null) return;

        _checkerLight.Changed += OnCheckerColourChanged;
        _checkerDark.Changed += OnCheckerColourChanged;
        BuildCheckerboard();
    }

    private void UnhookCheckerboard()
    {
        if (_checkerLight is not null) _checkerLight.Changed -= OnCheckerColourChanged;
        if (_checkerDark is not null) _checkerDark.Changed -= OnCheckerColourChanged;
        _checkerLight = _checkerDark = null;
    }

    private void OnCheckerColourChanged(object? sender, EventArgs e) => BuildCheckerboard();

    private void BuildCheckerboard()
    {
        if (_checkerLight is null || _checkerDark is null) return;

        const double square = 8;
        var drawing = new DrawingGroup();
        drawing.Children.Add(new GeometryDrawing(
            new SolidColorBrush(_checkerLight.Color),
            null, new RectangleGeometry(new Rect(0, 0, square * 2, square * 2))));
        drawing.Children.Add(new GeometryDrawing(
            new SolidColorBrush(_checkerDark.Color),
            null, new RectangleGeometry(new Rect(0, 0, square, square))));
        drawing.Children.Add(new GeometryDrawing(
            new SolidColorBrush(_checkerDark.Color),
            null, new RectangleGeometry(new Rect(square, square, square, square))));

        ImageBackdrop.Background = new DrawingBrush(drawing)
        {
            TileMode = TileMode.Tile,
            Viewport = new Rect(0, 0, square * 2, square * 2),
            ViewportUnits = BrushMappingMode.Absolute,
            Stretch = Stretch.None,
        };
    }

    // --- media ---

    private void StartOrStopMedia()
    {
        if (_model?.MediaSource is { } source)
        {
            MediaView.Source = source;
            MediaView.Play();
            _playing = true;
            PlayPause.Content = "❚❚";
            _mediaTick.Start();
        }
        else
        {
            _mediaTick.Stop();
            _playing = false;
            MediaView.Close();   // releases the file the moment the selection moves on
            MediaView.Source = null;
            Seek.Value = 0;
            TimeText.Text = "";
        }
    }

    private void PlayPause_Click(object sender, RoutedEventArgs e)
    {
        if (_playing) { MediaView.Pause(); PlayPause.Content = "▶"; }
        else { MediaView.Play(); PlayPause.Content = "❚❚"; }
        _playing = !_playing;
    }

    private void Media_Opened(object sender, RoutedEventArgs e)
    {
        Seek.Maximum = MediaView.NaturalDuration.HasTimeSpan
            ? MediaView.NaturalDuration.TimeSpan.TotalSeconds
            : 0;
        UpdateTimeText();
    }

    private void Media_Ended(object sender, RoutedEventArgs e)
    {
        // Rewound rather than closed: pressing play again should not have to reopen the file.
        MediaView.Position = TimeSpan.Zero;
        MediaView.Pause();
        _playing = false;
        PlayPause.Content = "▶";
    }

    /// <summary>A missing codec is a message, not a crash. N editions of Windows ship without the
    /// media stack at all, and the poster frame is still worth showing.</summary>
    private void Media_Failed(object sender, ExceptionRoutedEventArgs e)
    {
        _mediaTick.Stop();
        _playing = false;
        TimeText.Text = "Cannot play this format";
    }

    private void MediaTick(object? sender, EventArgs e)
    {
        if (_seeking) return;
        Seek.Value = MediaView.Position.TotalSeconds;
        UpdateTimeText();
    }

    private void UpdateTimeText()
    {
        var total = MediaView.NaturalDuration.HasTimeSpan ? MediaView.NaturalDuration.TimeSpan : TimeSpan.Zero;
        TimeText.Text = $"{Clock(MediaView.Position)} / {Clock(total)}";
    }

    private static string Clock(TimeSpan t) =>
        t.TotalHours >= 1 ? t.ToString(@"h\:mm\:ss") : t.ToString(@"m\:ss");

    private void Seek_DragStarted(object sender, DragStartedEventArgs e) => _seeking = true;

    private void Seek_DragCompleted(object sender, DragCompletedEventArgs e)
    {
        _seeking = false;
        MediaView.Position = TimeSpan.FromSeconds(Seek.Value);
        UpdateTimeText();
    }
}
