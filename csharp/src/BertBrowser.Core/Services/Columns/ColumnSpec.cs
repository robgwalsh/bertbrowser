namespace BertBrowser.Core.Services.Columns;

/// <summary>Where a column's value comes from.</summary>
/// <remarks>
/// This is an enum because it lives on <see cref="ColumnSpec"/>, which is catalogue data and is
/// <b>never serialized</b>. Do not move it onto <see cref="ColumnSetting"/>: settings.json is
/// written by plain <c>System.Text.Json</c> with no converters, so an enum there would be persisted
/// as an integer and a reordering of the members would silently change what an old file means.
/// </remarks>
public enum ColumnKind
{
    /// <summary>Answered from the row itself — no shell call, no I/O.</summary>
    BuiltIn,

    /// <summary>Read from the Windows property system, keyed by canonical name.</summary>
    ShellProperty,
}

/// <summary>
/// What a column <em>is</em>: immutable catalogue data, the same on every machine and in every tab.
/// </summary>
/// <param name="Id">
/// The identity persisted in settings. A built-in's id is a bare word (<c>Modified</c>); a shell
/// property's id <b>is its canonical name</b> (<c>System.Photo.DateTaken</c>). The two spaces cannot
/// collide because a canonical name always contains a dot and a built-in id never does — which is
/// what lets <see cref="ColumnLayoutRules"/> tell "a built-in from a newer version, drop it" from
/// "a property this machine may not have, keep it and render blank".
/// </param>
/// <param name="Header">
/// The column header. For a built-in this is the header. For a shell property it is only a
/// <em>fallback</em>: the real one is the localised name from <c>IPropertyDescription.GetDisplayName</c>,
/// resolved once when the column is added. Hard-coding "Dimensions" here would work perfectly on the
/// machine it was written on and read as gibberish on a German Windows — the lesson
/// <see cref="Preview.PreviewMetadata"/> already records.
/// </param>
/// <param name="Sortable">
/// False only for Match. Sorting a result set by the line number a needle was found on means nothing
/// across different files. Until columns were generated this was expressed by giving that column no
/// <c>Tag</c> for the click handler to parse; now that every column has an id, it has to be said.
/// </param>
public sealed record ColumnSpec(
    string Id,
    string Header,
    ColumnKind Kind,
    double DefaultWidth,
    bool RightAligned = false,
    bool Sortable = true,
    string Group = "");

/// <summary>
/// What the view builds: a spec, the width it should get, and whether the app put it there rather
/// than the user.
/// </summary>
/// <param name="Injected">
/// True for Folder and Match, which follow the list's mode rather than anyone's choice and are never
/// persisted. <see cref="ColumnLayoutRules.CaptureOrder"/> drops them on the way back to settings.
/// </param>
public sealed record ResolvedColumn(ColumnSpec Spec, double Width, bool Injected = false)
{
    public string Id => Spec.Id;
}
