namespace BertBrowser.Core.Services.Columns;

/// <summary>
/// What the user chose: one column, in order, with the width they dragged it to.
/// </summary>
/// <remarks>
/// <para>
/// Plain mutable get/set so settings.json stays hand-editable, the way
/// <see cref="NewItem.NewFileTemplate"/> is.
/// </para>
/// <para>
/// There is deliberately <b>no <c>Enabled</c> flag</b>, which is where this differs from
/// <c>NewFileTemplate</c>. There the list is a menu and "listed but switched off" is a real state
/// worth keeping. Here presence in the list <em>is</em> enabledness, and a flag would be a second
/// way to say the same thing — two sources of truth for one fact, which is the failure
/// <c>ResolvedNewFileTypes</c> exists to prevent.
/// </para>
/// </remarks>
public sealed class ColumnSetting
{
    public string Id { get; set; } = "";

    /// <summary>Width in device-independent pixels. Anything unusable — a hand-edited absurdity, a
    /// <c>NaN</c> from an auto-sized column — is repaired by
    /// <see cref="ColumnLayoutRules.SaneWidth"/> rather than reaching WPF.</summary>
    public double Width { get; set; }

    public ColumnSetting() { }

    public ColumnSetting(string id, double width)
    {
        Id = id;
        Width = width;
    }

    public ColumnSetting Copy() => new(Id, Width);
}

/// <summary>
/// The rule separating the two id spaces, kept here so it can be tested in a project that cannot
/// call <c>propsys.dll</c>.
/// </summary>
public static class ColumnId
{
    /// <summary>A registry-backed canonical name is nothing like this long; the cap is what stops a
    /// hand-edited settings file putting an arbitrary string into a header.</summary>
    public const int MaxLength = 128;

    /// <summary>
    /// Whether an id looks like a Windows canonical property name (<c>System.Photo.DateTaken</c>)
    /// rather than one of this app's built-in column ids.
    /// </summary>
    /// <remarks>
    /// The distinction decides what happens to an id the catalogue does not know. A built-in id it
    /// does not recognise came from a newer build and names a column this one cannot render, so it is
    /// dropped. A canonical name it does not recognise may be a perfectly good property this machine
    /// has no handler for, so it is kept and renders blank — unknown, never wrong.
    /// </remarks>
    public static bool LooksCanonical(string? id)
    {
        if (string.IsNullOrEmpty(id) || id.Length > MaxLength) return false;
        if (id[0] == '.' || id[^1] == '.') return false;

        var dot = false;
        foreach (var c in id)
        {
            if (c == '.') { dot = true; continue; }
            if (!char.IsAsciiLetterOrDigit(c) && c != '_') return false;
        }
        return dot;
    }
}
