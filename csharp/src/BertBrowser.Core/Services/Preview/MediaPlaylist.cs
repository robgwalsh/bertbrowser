namespace BertBrowser.Core.Services.Preview;

/// <summary>
/// What the preview pane plays after the current file, whether by its skip button or because the
/// current one ended with auto-advance on.
/// </summary>
/// <remarks>
/// The list is the file list's rows in the order the user sees them, so "next" follows whatever
/// sort is showing rather than the directory's own order. A video continues to the next video and
/// a song to the next song: a folder of clips with a soundtrack beside them should not break into
/// the MP3 halfway through. There is no wrap-around — reaching the end stops, the way a playlist
/// does, rather than starting the folder over while nobody is watching.
/// </remarks>
public static class MediaPlaylist
{
    /// <param name="names">The rows' names in display order. Folders should be passed as null, or
    /// left out, since a folder named <c>clip.mp4</c> is not a video.</param>
    /// <param name="current">Index of the file playing now.</param>
    /// <returns>The index to play next, or -1 when nothing after <paramref name="current"/> fits.</returns>
    public static int Next(IReadOnlyList<string?> names, int current)
    {
        if (current < 0 || current >= names.Count || names[current] is not { } playing) return -1;
        if (PreviewClassifier.KindFor(playing) != PreviewKind.Media) return -1;

        var video = PreviewClassifier.IsVideo(playing);
        for (var i = current + 1; i < names.Count; i++)
        {
            if (names[i] is { } name
                && PreviewClassifier.KindFor(name) == PreviewKind.Media
                && PreviewClassifier.IsVideo(name) == video)
                return i;
        }
        return -1;
    }
}
