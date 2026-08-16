namespace BertBrowser.Core.Services.Mft;

/// <summary>
/// An <see cref="IMftIndexService"/> that indexes nothing and reports nothing.
/// </summary>
/// <remarks>
/// <para>
/// Two callers want this. The UI harness must never raise an elevation prompt — the whole point of
/// running the window offscreen is that nothing appears on the user's desktop, and a UAC dialog is
/// the one thing offscreen cannot fix. And tests want the crawl and live-scan fallbacks exercised
/// without a volume to index.
/// </para>
/// <para>
/// Reporting nothing indexed is the honest answer rather than a convenient one: it is exactly what
/// a non-NTFS machine, a still-indexing one, and a declined elevation prompt all look like, so
/// every caller downstream is already required to handle it.
/// </para>
/// </remarks>
public sealed class NullMftIndexService : IMftIndexService
{
    public void Start() { }
    public bool AnyIndexed => false;
    public bool IsBuilding => false;
    public IReadOnlyCollection<string> BuildingDrives => [];
    public bool IsIndexed(string pathKey) => false;
    public string StatusText => "";
    public bool CanRetry => false;
    public void Retry() { }
    public event Action<string>? IndexRefreshed { add { } remove { } }
    public event Action? StatusChanged { add { } remove { } }
    public void Dispose() { }
}
