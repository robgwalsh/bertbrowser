using System.Globalization;
using System.Windows;
using System.Windows.Media;

namespace BertBrowser.App.Views;

/// <summary>
/// The line numbers beside a text preview, drawn only where they can be seen.
/// </summary>
/// <remarks>
/// This was a <c>TextBlock</c> holding every number, slid up and down behind a clip as the text
/// scrolled. A text block lays out all of its lines to draw any of them, and for a five-thousand-
/// line preview that was 0.9 s of the 1.4 s the file took to appear — more than the text itself.
/// Every line is the same height, so which numbers are on screen is arithmetic, and this draws
/// those thirty or so and nothing else.
/// </remarks>
public sealed class LineNumberGutter : FrameworkElement
{
    public static readonly DependencyProperty ForegroundProperty = DependencyProperty.Register(
        nameof(Foreground), typeof(Brush), typeof(LineNumberGutter),
        new FrameworkPropertyMetadata(null, FrameworkPropertyMetadataOptions.AffectsRender));

    private int _lineCount;
    private int _firstNumber = 1;
    private double _offset;

    public Brush? Foreground
    {
        get => (Brush?)GetValue(ForegroundProperty);
        set => SetValue(ForegroundProperty, value);
    }

    public FontFamily FontFamily { get; set; } = new("Consolas");

    public double FontSize { get; set; } = 12;

    /// <summary>The height of one line of the text this stands beside.</summary>
    public double LineHeight { get; set; } = 16;

    /// <summary>What to number: <paramref name="lineCount"/> lines, starting at
    /// <paramref name="firstNumber"/>, scrolled back to the top.</summary>
    public void Show(int lineCount, int firstNumber = 1)
    {
        _lineCount = Math.Max(0, lineCount);
        _firstNumber = firstNumber;
        _offset = 0;
        InvalidateMeasure();
        InvalidateVisual();
    }

    /// <summary>How far the text beside it has scrolled.</summary>
    public double Offset
    {
        get => _offset;
        set
        {
            if (_offset.Equals(value)) return;
            _offset = value;
            InvalidateVisual();
        }
    }

    /// <summary>As wide as the last number — in a fixed-pitch face the longest is the widest —
    /// and no taller than it is given: the lines it does not draw take no room.</summary>
    protected override Size MeasureOverride(Size availableSize) =>
        _lineCount == 0
            ? default
            : new Size(Math.Ceiling(Format(_firstNumber + _lineCount - 1).WidthIncludingTrailingWhitespace), 0);

    protected override void OnRender(DrawingContext drawingContext)
    {
        if (_lineCount == 0 || LineHeight <= 0) return;

        // One line either side of the edges, so a number half scrolled into the margin is drawn.
        var first = Math.Max(0, (int)(_offset / LineHeight) - 1);
        var last = Math.Min(_lineCount - 1, (int)((_offset + ActualHeight) / LineHeight) + 1);

        for (var line = first; line <= last; line++)
        {
            var text = Format(_firstNumber + line);
            drawingContext.DrawText(
                text, new Point(ActualWidth - text.WidthIncludingTrailingWhitespace, line * LineHeight - _offset));
        }
    }

    private FormattedText Format(int number) =>
        new(number.ToString(CultureInfo.InvariantCulture), CultureInfo.InvariantCulture, FlowDirection.LeftToRight,
            new Typeface(FontFamily, FontStyles.Normal, FontWeights.Normal, FontStretches.Normal),
            FontSize, Foreground, VisualTreeHelper.GetDpi(this).PixelsPerDip)
        {
            // Block line height, as the text has it, so each number sits on its line's baseline.
            LineHeight = LineHeight,
        };
}
