namespace BertBrowser.Core.Models;

/// <summary>
/// A width:height ratio, written "4:3" — the shape of a thumbnail tile. Lives in Core, and is a
/// parsed value rather than a raw string, because the setting is stored in settings.json where a
/// user can type anything into it. Parsing therefore <b>never throws</b>: an unreadable, absurd or
/// missing value resolves to <see cref="Default"/> and the tiles still draw, the same contract the
/// theme resolver keeps.
/// </summary>
public readonly record struct AspectRatio
{
    /// <summary>Widest term either side may take. Bounds a hand-edited settings file to shapes
    /// that are merely odd rather than degenerate (a 400:1 tile is a line).</summary>
    public const int MaxTerm = 100;

    private AspectRatio(int width, int height)
    {
        Width = width;
        Height = height;
    }

    public int Width { get; }

    public int Height { get; }

    /// <summary>4:3, matching the shape most photos and video thumbnails already are.</summary>
    public static AspectRatio Default { get; } = new(4, 3);

    /// <summary>What the settings picker offers, in the order it offers them. A value the user
    /// typed into settings.json by hand still works — it is simply not in this list.</summary>
    public static IReadOnlyList<AspectRatio> Presets { get; } = new[]
    {
        new AspectRatio(1, 1),
        new AspectRatio(4, 3),
        new AspectRatio(3, 2),
        new AspectRatio(16, 10),
        new AspectRatio(16, 9),
        new AspectRatio(3, 4),
        new AspectRatio(2, 3),
    };

    /// <summary>Parses "W:H", tolerating surrounding and inner whitespace. False for anything else,
    /// including zero or negative terms and terms above <see cref="MaxTerm"/>.</summary>
    public static bool TryParse(string? text, out AspectRatio value)
    {
        value = default;
        if (string.IsNullOrWhiteSpace(text)) return false;

        var parts = text.Split(':');
        if (parts.Length != 2) return false;

        if (!int.TryParse(parts[0].Trim(), out var width) ||
            !int.TryParse(parts[1].Trim(), out var height))
            return false;

        if (width is < 1 or > MaxTerm || height is < 1 or > MaxTerm) return false;

        value = new AspectRatio(width, height);
        return true;
    }

    /// <summary>The parsed ratio, or <see cref="Default"/> for anything unusable.</summary>
    public static AspectRatio Parse(string? text) => TryParse(text, out var value) ? value : Default;

    /// <summary>The height a tile of <paramref name="width"/> pixels should be. A default-constructed
    /// ratio (0:0 — reachable, as this is a struct) is treated as square rather than dividing by
    /// zero and handing WPF a NaN.</summary>
    public double HeightFor(double width) => Width > 0 ? width * Height / Width : width;

    public override string ToString() => $"{Width}:{Height}";
}
