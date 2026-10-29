using BertBrowser.Core.Services.Preview;

namespace BertBrowser.Core.Services.Compare;

/// <summary>How a pair of files should be shown side by side.</summary>
public enum FileCompareMode
{
    /// <summary>A line diff.</summary>
    Text,

    /// <summary>A hex dump around the first differing byte.</summary>
    Binary,
}

/// <summary>What one side of a pair turned out to be, as far as anything can tell before reading.</summary>
/// <param name="Name">Its file name, for the extension table.</param>
/// <param name="LooksLikeText">
/// Whether decoding the front of it produced convincing text. From
/// <see cref="TextPreviewReader.IsConvincingText"/>, so the pane and the comparison cannot come to
/// different conclusions about what a file is.
/// </param>
public readonly record struct FileCompareSide(string Name, bool LooksLikeText);

/// <summary>
/// Deciding whether two files are worth diffing as text.
/// </summary>
/// <remarks>
/// A rule rather than an <c>if</c> in the window, so it can be tested — and so the answer to
/// "why is this showing hex?" is a function with a name rather than a condition buried in a view.
/// </remarks>
public static class FileComparePlan
{
    /// <summary>
    /// <see cref="FileCompareMode.Text"/> only when <em>both</em> sides can be read as text.
    /// </summary>
    /// <remarks>
    /// Both, not either. A <c>.txt</c> against a <c>.exe</c> diffed as text would render one side as
    /// a screen of mojibake and invite the reader to conclude something about it; the honest answer
    /// for a mixed pair is the one that works for the harder side.
    ///
    /// A side counts as text if its extension says so — the same table the preview pane trusts, so
    /// an empty <c>.cs</c> or one full of unusual characters is still code — or if, failing that,
    /// what was decoded is convincing on its own. That second rung is what makes an extensionless
    /// <c>README</c> or a <c>Dockerfile</c> work.
    /// </remarks>
    public static FileCompareMode For(FileCompareSide left, FileCompareSide right, bool forceText = false)
    {
        if (forceText) return FileCompareMode.Text;

        return IsText(left) && IsText(right) ? FileCompareMode.Text : FileCompareMode.Binary;
    }

    /// <remarks>
    /// <see cref="PreviewClassifier.KindFor"/> answers <see cref="PreviewKind.Text"/> for a name
    /// with no extension at all, on the grounds that such a file is usually a Dockerfile or a
    /// LICENSE and reading one as text is harmless. That reasoning is sound for a preview and does
    /// not carry here: a wrong guess in a pane is one panel of mojibake the reader can dismiss,
    /// while a wrong guess here produces two of them side by side with differences marked between
    /// them, which reads as a finding. So the extensionless case falls through to what was actually
    /// decoded, which by then is known — and every other answer the classifier gives is used as it
    /// stands, so there is still one extension table.
    /// </remarks>
    private static bool IsText(FileCompareSide side)
    {
        var named = Path.GetExtension(side.Name).Length > 0
            && PreviewClassifier.KindFor(side.Name) is PreviewKind.Text;

        return named || side.LooksLikeText;
    }
}
