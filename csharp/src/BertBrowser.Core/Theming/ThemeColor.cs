using System.Globalization;

namespace BertBrowser.Core.Theming;

/// <summary>
/// A straight ARGB colour. Deliberately not <c>System.Windows.Media.Color</c>: themes are resolved,
/// validated and contrast-checked in Core, which has no UI dependencies, and the App project turns
/// these into brushes at the last moment.
/// </summary>
public readonly record struct ThemeColor(byte A, byte R, byte G, byte B)
{
    public static ThemeColor FromRgb(byte r, byte g, byte b) => new(0xFF, r, g, b);

    /// <summary>
    /// Parses <c>#RGB</c>, <c>#RGBA</c>, <c>#RRGGBB</c> or <c>#AARRGGBB</c>, with or without the
    /// leading <c>#</c> and in either case. Never throws — a theme file is user-editable text.
    /// </summary>
    public static bool TryParse(string? text, out ThemeColor color)
    {
        color = default;
        if (string.IsNullOrWhiteSpace(text)) return false;

        var span = text.AsSpan().Trim();
        if (span.Length > 0 && span[0] == '#') span = span[1..];

        // Short forms repeat each nibble, so #ABC is #AABBCC — the CSS rule, and what anyone
        // hand-editing a theme file will expect.
        switch (span.Length)
        {
            case 3:
                return TryNibbles(span, 0xFF, out color);
            case 4:
                if (!TryHex(span[0], out var sa)) return false;
                return TryNibbles(span[1..], (byte)(sa * 17), out color);
            case 6:
                return TryBytes(span, 0xFF, out color);
            case 8:
                if (!TryByte(span, out var a)) return false;
                return TryBytes(span[2..], a, out color);
            default:
                return false;
        }
    }

    /// <summary>Parse or throw. For built-in theme data, where a bad literal is a bug, not input.</summary>
    public static ThemeColor Parse(string text) =>
        TryParse(text, out var color) ? color : throw new FormatException($"'{text}' is not a colour.");

    /// <summary>Round-trips through <see cref="TryParse"/>. Opaque colours omit the alpha pair.</summary>
    public string ToHex() => A == 0xFF
        ? $"#{R:X2}{G:X2}{B:X2}"
        : $"#{A:X2}{R:X2}{G:X2}{B:X2}";

    public override string ToString() => ToHex();

    /// <summary>
    /// This colour composited over an opaque <paramref name="background"/>. Contrast checks need it:
    /// a token like the scrollbar thumb is only ever seen blended with what sits behind it.
    /// </summary>
    public ThemeColor CompositeOver(ThemeColor background)
    {
        if (A == 0xFF) return this;
        var alpha = A / 255.0;
        static byte Mix(byte fg, byte bg, double a) => (byte)Math.Round(fg * a + bg * (1 - a));
        return new ThemeColor(0xFF, Mix(R, background.R, alpha), Mix(G, background.G, alpha), Mix(B, background.B, alpha));
    }

    /// <summary>WCAG 2.1 relative luminance (0 = black, 1 = white), ignoring alpha.</summary>
    public double RelativeLuminance()
    {
        static double Channel(byte v)
        {
            var c = v / 255.0;
            return c <= 0.04045 ? c / 12.92 : Math.Pow((c + 0.055) / 1.055, 2.4);
        }
        return 0.2126 * Channel(R) + 0.7152 * Channel(G) + 0.0722 * Channel(B);
    }

    /// <summary>Hue in degrees [0,360), saturation and value in [0,1]. Alpha is carried separately.</summary>
    public (double H, double S, double V) ToHsv()
    {
        double r = R / 255.0, g = G / 255.0, b = B / 255.0;
        var max = Math.Max(r, Math.Max(g, b));
        var min = Math.Min(r, Math.Min(g, b));
        var delta = max - min;

        double h;
        if (delta == 0) h = 0;
        else if (max == r) h = 60 * (((g - b) / delta + 6) % 6);
        else if (max == g) h = 60 * ((b - r) / delta + 2);
        else h = 60 * ((r - g) / delta + 4);

        return (h, max == 0 ? 0 : delta / max, max);
    }

    public static ThemeColor FromHsv(double h, double s, double v, byte a = 0xFF)
    {
        h = ((h % 360) + 360) % 360;
        s = Math.Clamp(s, 0, 1);
        v = Math.Clamp(v, 0, 1);

        var c = v * s;
        var x = c * (1 - Math.Abs(h / 60 % 2 - 1));
        var m = v - c;

        var (r, g, b) = (int)(h / 60) switch
        {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };

        static byte Scale(double v) => (byte)Math.Round(Math.Clamp(v, 0, 1) * 255);
        return new ThemeColor(a, Scale(r + m), Scale(g + m), Scale(b + m));
    }

    private static bool TryNibbles(ReadOnlySpan<char> span, byte alpha, out ThemeColor color)
    {
        color = default;
        if (!TryHex(span[0], out var r) || !TryHex(span[1], out var g) || !TryHex(span[2], out var b))
            return false;
        color = new ThemeColor(alpha, (byte)(r * 17), (byte)(g * 17), (byte)(b * 17));
        return true;
    }

    private static bool TryBytes(ReadOnlySpan<char> span, byte alpha, out ThemeColor color)
    {
        color = default;
        if (!TryByte(span, out var r) || !TryByte(span[2..], out var g) || !TryByte(span[4..], out var b))
            return false;
        color = new ThemeColor(alpha, r, g, b);
        return true;
    }

    private static bool TryByte(ReadOnlySpan<char> span, out byte value) =>
        byte.TryParse(span[..2], NumberStyles.HexNumber, CultureInfo.InvariantCulture, out value);

    private static bool TryHex(char c, out byte value)
    {
        value = 0;
        if (c is >= '0' and <= '9') value = (byte)(c - '0');
        else if (c is >= 'a' and <= 'f') value = (byte)(c - 'a' + 10);
        else if (c is >= 'A' and <= 'F') value = (byte)(c - 'A' + 10);
        else return false;
        return true;
    }
}
