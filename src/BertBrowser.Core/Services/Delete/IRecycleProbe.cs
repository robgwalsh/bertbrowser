namespace BertBrowser.Core.Services.Delete;

/// <summary>
/// Whether a path's volume has a Recycle Bin that will actually take something.
/// </summary>
/// <remarks>
/// Asked by the planner so the confirmation can be honest, and again by the executor against live
/// state. It is a separate interface from <see cref="IDeleteProbe"/> because the answer comes from
/// the shell rather than the filesystem, and because the rule that routes an item to the holding
/// folder instead needs testing against volumes — network shares, media with the bin disabled —
/// that cannot be conjured up in a unit test.
/// </remarks>
public interface IRecycleProbe
{
    /// <summary>False for a network share, a volume whose bin is turned off, or anything else the
    /// shell would silently erase rather than hold.</summary>
    bool CanRecycle(string path);
}

/// <summary>
/// The answer when there is no Recycle Bin to ask about — Core on its own, and any test that has
/// not opted in. Everything routes to the holding folder, which is the behaviour that predates the
/// bin and never loses data.
/// </summary>
public sealed class NoRecycleProbe : IRecycleProbe
{
    public static NoRecycleProbe Instance { get; } = new();

    public bool CanRecycle(string path) => false;
}
