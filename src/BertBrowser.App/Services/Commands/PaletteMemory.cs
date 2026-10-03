namespace BertBrowser.App.Services.Commands;

/// <summary>
/// What the command palette remembers about the person using it: what they pinned, and what they
/// ran since the window opened.
/// </summary>
/// <remarks>
/// <b>Recents are never written to disk.</b> A list of what somebody ran is a history, and this
/// app's rule for anything that keeps one is that it is off until asked for. Kept in memory only,
/// it is gone when the app closes — the same footing the undo history stands on — and so needs no
/// switch and no page. Pins are different in kind: each one is there because a button was pressed
/// to put it there, so they are a preference and are saved like one.
/// </remarks>
public sealed class PaletteMemory
{
    /// <summary>Enough to fill the palette's "Recent" section several times over; older ones
    /// are simply forgotten.</summary>
    private const int MaxRecents = 20;

    private readonly AppSettings _settings;
    private readonly List<string> _recents = [];

    public PaletteMemory(AppSettings settings) => _settings = settings;

    public IReadOnlyList<string> Pins => _settings.PalettePins;

    /// <summary>Most recent first.</summary>
    public IReadOnlyList<string> Recents => _recents;

    public bool IsPinned(string id) => _settings.PalettePins.Contains(id);

    public void TogglePin(string id)
    {
        if (!_settings.PalettePins.Remove(id)) _settings.PalettePins.Add(id);
        _settings.Save();
    }

    public void NoteRun(string id)
    {
        _recents.Remove(id);
        _recents.Insert(0, id);
        if (_recents.Count > MaxRecents) _recents.RemoveAt(_recents.Count - 1);
    }
}
