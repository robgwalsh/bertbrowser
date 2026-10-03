using System.Windows;
using System.Windows.Controls;
using System.Windows.Documents;
using BertBrowser.Core.Services.Commands;

namespace BertBrowser.App.Views;

/// <summary>
/// A text block that draws some stretches of its text heavier — the letters of a command's name
/// that matched what was typed.
/// </summary>
/// <remarks>
/// Weight rather than colour, on purpose. The row this sits in changes background when it is
/// selected and is dimmed when the command cannot run, and a highlight colour would need to clear
/// contrast against every one of those in every theme; a heavier stroke of the colour the row
/// already uses is readable wherever the row is.
/// </remarks>
public sealed class HighlightTextBlock : TextBlock
{
    public static readonly DependencyProperty SourceProperty = DependencyProperty.Register(
        nameof(Source), typeof(string), typeof(HighlightTextBlock),
        new PropertyMetadata("", (d, _) => ((HighlightTextBlock)d).Rebuild()));

    public static readonly DependencyProperty RangesProperty = DependencyProperty.Register(
        nameof(Ranges), typeof(IReadOnlyList<MatchRange>), typeof(HighlightTextBlock),
        new PropertyMetadata(null, (d, _) => ((HighlightTextBlock)d).Rebuild()));

    public string Source
    {
        get => (string)GetValue(SourceProperty);
        set => SetValue(SourceProperty, value);
    }

    public IReadOnlyList<MatchRange>? Ranges
    {
        get => (IReadOnlyList<MatchRange>?)GetValue(RangesProperty);
        set => SetValue(RangesProperty, value);
    }

    private void Rebuild()
    {
        Inlines.Clear();
        var text = Source ?? "";
        var at = 0;

        foreach (var range in Ranges ?? [])
        {
            // Ranges arrive sorted and merged; anything past the end is ignored rather than thrown
            // on, since a row can be redrawn between its text and its ranges changing.
            if (range.Start < at || range.Start >= text.Length) continue;
            var length = Math.Min(range.Length, text.Length - range.Start);

            if (range.Start > at) Inlines.Add(new Run(text[at..range.Start]));
            Inlines.Add(new Run(text.Substring(range.Start, length)) { FontWeight = FontWeights.Bold });
            at = range.Start + length;
        }

        if (at < text.Length) Inlines.Add(new Run(text[at..]));
    }
}
