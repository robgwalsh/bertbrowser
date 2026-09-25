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

    /// <summary>What <c>UnloadedBehavior="Close"</c> used to do, done here instead: that setting
    /// would also fire when the media host is lent to the full-screen window, and close the video
    /// on its way into it.</summary>
    private void OnUnloaded(object sender, RoutedEventArgs e)
    {
        ExitFullScreen();
        _mediaTick.Stop();
        MediaView.Close();
        SetPlaying(false);
    }

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
        ExitFullScreen();
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
            case nameof(PreviewPaneViewModel.Image):
                // A new picture starts fitted, whether or not the view model's flag moved. Set
                // directly: layout has not measured the new image yet, so there is nothing to clamp to.
                ImageScale.ScaleX = ImageScale.ScaleY = 1;
                ImageOffset.X = ImageOffset.Y = 0;
                UpdatePanCursor();
                break;
            case nameof(PreviewPaneViewModel.MediaSource):
                StartOrStopMedia();
                break;
            case nameof(PreviewPaneViewModel.StateName):
                // The selection moved to something that is not media: nothing is left to show full
                // screen. Not while loading — advancing to the next video passes through that.
                if (_fullScreen is not null && _model is { IsLoading: false, HasMedia: false })
                    ExitFullScreen();
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

    /// <summary>Set while the wheel moves the view model off "fit", so the property change that
    /// follows does not snap the zoom to 100%.</summary>
    private bool _zoomingByWheel;

    private Point? _panStart;
    private Point _panStartOffset;

    private double ZoomScale => ImageScale.ScaleX;

    /// <summary>How much the fitted image must be scaled to show one image pixel per device-independent
    /// pixel — <see cref="BitmapSource.Width"/> is already in DIPs. Below 1 for an image that fitting
    /// blew up.</summary>
    private double OneToOneScale =>
        PreviewImage.Source is BitmapSource bitmap && PreviewImage.ActualWidth > 0
            ? bitmap.Width / PreviewImage.ActualWidth
            : 1;

    /// <summary>Fit is the floor, unless 100% is smaller than fit — then the wheel may go down to it.</summary>
    private double MinZoomScale => Math.Min(1, OneToOneScale);

    private void Image_MouseDown(object sender, MouseButtonEventArgs e)
    {
        if (e.ClickCount == 2)
        {
            _model?.ToggleFitCommand.Execute(null);
            e.Handled = true;
            return;
        }
        if (!CanPan()) return;

        _panStart = e.GetPosition(ImageBackdrop);
        _panStartOffset = new Point(ImageOffset.X, ImageOffset.Y);
        ImageBackdrop.CaptureMouse();
        e.Handled = true;
    }

    private void Image_MouseMove(object sender, MouseEventArgs e)
    {
        if (_panStart is not { } start) return;
        var now = e.GetPosition(ImageBackdrop);
        SetImageTransform(ZoomScale,
            _panStartOffset.X + (now.X - start.X),
            _panStartOffset.Y + (now.Y - start.Y));
    }

    private void Image_MouseUp(object sender, MouseButtonEventArgs e)
    {
        if (_panStart is null) return;
        ImageBackdrop.ReleaseMouseCapture();
        e.Handled = true;
    }

    private void Image_LostCapture(object sender, MouseEventArgs e)
    {
        _panStart = null;
        UpdatePanCursor();
    }

    /// <summary>Zooms about the point under the cursor, so what the user is pointing at stays put.</summary>
    private void Image_MouseWheel(object sender, MouseWheelEventArgs e)
    {
        if (PreviewImage.Source is null || PreviewImage.ActualWidth <= 0) return;
        e.Handled = true;

        var oldScale = ZoomScale;
        var newScale = ImageZoom.Step(oldScale, e.Delta / 120.0, MinZoomScale);
        if (newScale == oldScale) return;

        ZoomAbout(e.GetPosition(PreviewImage), newScale);

        if (_model is { FitImageToPane: true })
        {
            _zoomingByWheel = true;
            try { _model.FitImageToPane = false; }
            finally { _zoomingByWheel = false; }
        }
    }

    /// <summary>The pane (or the image within it) resized, so layout refitted the image under the
    /// transform; only the pan limits need recomputing.</summary>
    private void ImageBackdrop_SizeChanged(object sender, SizeChangedEventArgs e) =>
        SetImageTransform(ZoomScale, ImageOffset.X, ImageOffset.Y);

    /// <summary>Fit is a scale of 1 — layout already fitted the image, up or down. 100% zooms about
    /// the middle of the pane to one image pixel per DIP.</summary>
    private void ApplyImageFit()
    {
        if (_zoomingByWheel) return;
        if (_model?.FitImageToPane != false)
        {
            SetImageTransform(1, 0, 0);
            return;
        }

        var centre = new Point(ImageBackdrop.ActualWidth / 2, ImageBackdrop.ActualHeight / 2);
        ZoomAbout(ImageBackdrop.TranslatePoint(centre, PreviewImage), ImageZoom.Clamp(OneToOneScale, OneToOneScale));
    }

    /// <param name="anchor">The image point to hold still, in the image's untransformed coordinates.</param>
    private void ZoomAbout(Point anchor, double newScale)
    {
        var oldScale = ZoomScale;
        SetImageTransform(newScale,
            ImageZoom.ZoomOffset(ImageOffset.X, anchor.X, oldScale, newScale),
            ImageZoom.ZoomOffset(ImageOffset.Y, anchor.Y, oldScale, newScale));
    }

    private void SetImageTransform(double scale, double x, double y)
    {
        // Layout's own placement of the fitted image, which the transform is applied on top of.
        var origin = VisualTreeHelper.GetOffset(PreviewImage);
        ImageScale.ScaleX = ImageScale.ScaleY = scale;
        ImageOffset.X = ImageZoom.ClampOffset(x, origin.X, PreviewImage.ActualWidth, scale, ImageBackdrop.ActualWidth);
        ImageOffset.Y = ImageZoom.ClampOffset(y, origin.Y, PreviewImage.ActualHeight, scale, ImageBackdrop.ActualHeight);
        UpdatePanCursor();
    }

    /// <summary>Only a picture bigger than the pane has anywhere to go.</summary>
    private bool CanPan() =>
        PreviewImage.ActualWidth * ZoomScale > ImageBackdrop.ActualWidth + 0.5
        || PreviewImage.ActualHeight * ZoomScale > ImageBackdrop.ActualHeight + 0.5;

    private void UpdatePanCursor() =>
        ImageBackdrop.Cursor = _panStart is not null ? Cursors.SizeAll
            : CanPan() ? Cursors.Hand
            : null;

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

    /// <summary>How often a drag of the seek bar re-positions the video. Every mouse-move would
    /// queue seeks faster than the decoder can land them, and the picture would trail the thumb.</summary>
    private const long ScrubIntervalMs = 60;

    /// <summary>Set while the tick moves the seek bar, so that move is not mistaken for the user's.</summary>
    private bool _tickMovingSeek;
    private bool _resumeAfterScrub;
    private long _lastScrubTick;

    private void StartOrStopMedia()
    {
        if (_model?.MediaSource is { } source)
        {
            MediaView.Source = source;
            MediaView.Play();
            SetPlaying(true);
            _mediaTick.Start();
        }
        else
        {
            _mediaTick.Stop();
            SetPlaying(false);
            MediaView.Close();   // releases the file the moment the selection moves on
            MediaView.Source = null;
            _tickMovingSeek = true;
            Seek.Value = 0;
            _tickMovingSeek = false;
            TimeText.Text = "";
        }
    }

    private void SetPlaying(bool playing)
    {
        _playing = playing;
        PlayPause.Content = FindResource(playing ? "Icon.Pause" : "Icon.Play");
    }

    private void TogglePlayback()
    {
        if (MediaView.Source is null) return;
        if (_playing) MediaView.Pause();
        else MediaView.Play();
        SetPlaying(!_playing);
    }

    private void PlayPause_Click(object sender, RoutedEventArgs e) => TogglePlayback();

    /// <summary>Once toggles playback (or starts it from the poster); twice toggles full screen. A
    /// double-click therefore pauses and resumes on its way, which is what every player does.</summary>
    private void VideoArea_MouseDown(object sender, MouseButtonEventArgs e)
    {
        if (_model is null) return;
        if (e.ClickCount == 2)
        {
            if (_model.MediaSource is not null) ToggleFullScreen();
        }
        else if (_model.MediaSource is null)
        {
            _model.PlayMediaCommand.Execute(null);
        }
        else
        {
            TogglePlayback();
        }
        e.Handled = true;
    }

    private void Media_Opened(object sender, RoutedEventArgs e)
    {
        Seek.Maximum = MediaView.NaturalDuration.HasTimeSpan
            ? MediaView.NaturalDuration.TimeSpan.TotalSeconds
            : 0;
        // Arrow keys and a click on the track move by these, so they scale with the video.
        Seek.SmallChange = Math.Max(1, Seek.Maximum / 200);
        Seek.LargeChange = Math.Max(5, Seek.Maximum / 20);
        UpdateTimeText(MediaView.Position);
    }

    private void Media_Ended(object sender, RoutedEventArgs e)
    {
        // Rewound rather than closed: pressing play again should not have to reopen the file. With
        // auto-advance on, the tab then selects the next video, whose arrival replaces this one;
        // at the end of the list nothing comes, and this rewound state is where it stops.
        MediaView.Position = TimeSpan.Zero;
        MediaView.Pause();
        SetPlaying(false);
        if (_model is { AutoAdvance: true }) _model.PlayNextCommand.Execute(null);
    }

    /// <summary>A missing codec is a message, not a crash. N editions of Windows ship without the
    /// media stack at all, and the poster frame is still worth showing.</summary>
    private void Media_Failed(object sender, ExceptionRoutedEventArgs e)
    {
        _mediaTick.Stop();
        SetPlaying(false);
        TimeText.Text = "Cannot play this format";
    }

    private void MediaTick(object? sender, EventArgs e)
    {
        if (_seeking) return;
        _tickMovingSeek = true;
        Seek.Value = MediaView.Position.TotalSeconds;
        _tickMovingSeek = false;
        UpdateTimeText(MediaView.Position);
    }

    private void UpdateTimeText(TimeSpan position)
    {
        var total = MediaView.NaturalDuration.HasTimeSpan ? MediaView.NaturalDuration.TimeSpan : TimeSpan.Zero;
        TimeText.Text = $"{Clock(position)} / {Clock(total)}";
    }

    private static string Clock(TimeSpan t) =>
        t.TotalHours >= 1 ? t.ToString(@"h\:mm\:ss") : t.ToString(@"m\:ss");

    /// <summary>
    /// Every move of the seek bar that is not the tick's is the user's, and seeks: a drag as it
    /// goes, a click on the track or an arrow key at once.
    /// </summary>
    /// <remarks>
    /// While dragging, the video is paused and <c>ScrubbingEnabled</c> has the decoder paint each
    /// frame it lands on — the picture follows the thumb. Seeks are throttled to
    /// <see cref="ScrubIntervalMs"/>; the drag's last position is always applied on release.
    /// </remarks>
    private void Seek_ValueChanged(object sender, RoutedPropertyChangedEventArgs<double> e)
    {
        if (_tickMovingSeek || MediaView.Source is null) return;
        var target = TimeSpan.FromSeconds(e.NewValue);
        UpdateTimeText(target);

        if (_seeking)
        {
            var now = Environment.TickCount64;
            if (now - _lastScrubTick < ScrubIntervalMs) return;
            _lastScrubTick = now;
        }
        MediaView.Position = target;
    }

    private void Seek_DragStarted(object sender, DragStartedEventArgs e)
    {
        _seeking = true;
        _resumeAfterScrub = _playing;
        if (_playing) MediaView.Pause();
    }

    private void Seek_DragCompleted(object sender, DragCompletedEventArgs e)
    {
        _seeking = false;
        if (MediaView.Source is null) return;
        MediaView.Position = TimeSpan.FromSeconds(Seek.Value);
        UpdateTimeText(MediaView.Position);
        if (_resumeAfterScrub) MediaView.Play();
    }

    // --- full screen ---

    /// <summary>The borderless window the media host is lent to, or null in the pane.</summary>
    private Window? _fullScreen;

    private void FullScreen_Click(object sender, RoutedEventArgs e) => ToggleFullScreen();

    private void ToggleFullScreen()
    {
        if (_fullScreen is null) EnterFullScreen();
        else ExitFullScreen();
    }

    /// <summary>
    /// Lends <c>MediaHost</c> — video, poster and transport together — to a borderless window
    /// covering the monitor the pane is on, so the one <see cref="MediaElement"/> keeps playing
    /// without a reopen and every control keeps its handler.
    /// </summary>
    private void EnterFullScreen()
    {
        if (_fullScreen is not null || _model is null || !IsLoaded) return;
        var owner = Window.GetWindow(this);

        // Placed on the pane's monitor before maximising, since maximising fills whichever monitor
        // the window's normal position is on.
        var centre = PointToScreen(new Point(ActualWidth / 2, ActualHeight / 2));
        if (PresentationSource.FromVisual(this)?.CompositionTarget is { } target)
            centre = target.TransformFromDevice.Transform(centre);

        MediaSlot.Child = new TextBlock
        {
            Text = "Playing full screen",
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
            Foreground = (Brush)FindResource(ThemeToken.TextMuted),
        };

        var window = new Window
        {
            Title = _model.Title,
            Owner = owner,
            WindowStyle = WindowStyle.None,
            ResizeMode = ResizeMode.NoResize,
            ShowInTaskbar = false,
            WindowStartupLocation = WindowStartupLocation.Manual,
            Left = centre.X - 50,
            Top = centre.Y - 50,
            Width = 100,
            Height = 100,
            DataContext = _model,
            Background = (Brush)FindResource(ThemeToken.WindowBackground),
            Content = MediaHost,
        };
        window.PreviewKeyDown += FullScreen_KeyDown;
        window.Closed += (_, _) => ReturnMediaHost();
        _fullScreen = window;
        FullScreenButton.Content = FindResource("Icon.ExitFullScreen");

        window.Show();
        window.WindowState = WindowState.Maximized;
        window.Activate();
    }

    private void ExitFullScreen() => _fullScreen?.Close();

    /// <summary>Runs from the window's <c>Closed</c>, so Alt+F4 and Esc both land here.</summary>
    private void ReturnMediaHost()
    {
        if (_fullScreen is not { } window) return;
        _fullScreen = null;
        window.Content = null;
        MediaSlot.Child = MediaHost;
        FullScreenButton.Content = FindResource("Icon.FullScreen");
        Window.GetWindow(this)?.Activate();
    }

    private void FullScreen_KeyDown(object sender, KeyEventArgs e)
    {
        switch (e.Key)
        {
            case Key.Escape:
            case Key.F11:
                ExitFullScreen();
                break;
            case Key.Space:
                TogglePlayback();
                break;
            case Key.Left:
                Seek.Value = Math.Max(0, Seek.Value - 5);
                break;
            case Key.Right:
                Seek.Value = Math.Min(Seek.Maximum, Seek.Value + 5);
                break;
            case Key.N:
                _model?.PlayNextCommand.Execute(null);
                break;
            default:
                return;
        }
        e.Handled = true;
    }
}
