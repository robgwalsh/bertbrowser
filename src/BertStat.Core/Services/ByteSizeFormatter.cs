namespace BertStat.Core.Services;

public static class ByteSizeFormatter
{
    private static readonly string[] Units = { "B", "KB", "MB", "GB", "TB", "PB" };

    /// <summary>Formats bytes the same way for files and directories, e.g. "1.4 MB".</summary>
    public static string Format(long bytes)
    {
        if (bytes < 0) return "—";
        double value = bytes;
        var unit = 0;
        while (value >= 1024 && unit < Units.Length - 1)
        {
            value /= 1024;
            unit++;
        }
        return unit == 0
            ? $"{bytes} B"
            : $"{value:0.#} {Units[unit]}";
    }
}
