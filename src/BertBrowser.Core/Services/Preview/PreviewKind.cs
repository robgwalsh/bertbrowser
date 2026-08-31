namespace BertBrowser.Core.Services.Preview;

/// <summary>How a file should be shown in the preview pane.</summary>
public enum PreviewKind
{
    /// <summary>Nothing to show — the selection is empty, or it was refused.</summary>
    None,

    /// <summary>Decoded to a bitmap and shown, with the shell as a fallback decoder.</summary>
    Image,

    /// <summary>Read as text and shown with syntax colouring.</summary>
    Text,

    /// <summary>Audio or video: a poster frame, and a transport the user must press.</summary>
    Media,

    /// <summary>Listed entry by entry, without extracting anything.</summary>
    Archive,

    /// <summary>Rendered as a specimen using the face in the file itself.</summary>
    Font,

    /// <summary>Everything else, including PDF and Office: whatever preview the shell can
    /// produce. Also the fall-back for an unrecognised extension, which is why an unknown file
    /// gets a real attempt rather than an immediate refusal.</summary>
    Document,

    /// <summary>A hex dump: offset, bytes, ASCII. Never chosen by the extension table — it is only
    /// ever what <see cref="PreviewMode.Hex"/> asked for.</summary>
    Hex,
}

/// <summary>What the user asked the pane to show, over the top of what the file claims to be.</summary>
/// <remarks>
/// The override is applied <em>after</em> the classifier's refusals, never before: a folder and a
/// cloud placeholder are still refused, because hexing a placeholder would trigger exactly the
/// multi-gigabyte download the pane exists to avoid.
/// </remarks>
public enum PreviewMode
{
    /// <summary>Let the extension decide, which is what the pane has always done.</summary>
    Auto,

    /// <summary>Decode the bytes as text whatever the file claims to be, and do not colour them.
    /// Total Commander's Lister calls this binary mode; it is how you read a PDF's header.</summary>
    Text,

    /// <summary>A hex dump.</summary>
    Hex,
}

/// <summary>Why there is no preview. <see cref="None"/> means there is one.</summary>
public enum PreviewRefusal
{
    None,

    /// <summary>Nothing is selected.</summary>
    NothingSelected,

    /// <summary>Several items are selected; the pane shows a summary instead.</summary>
    MultipleSelected,

    /// <summary>A folder — counts and the cached size, never a walk.</summary>
    Folder,

    /// <summary>Bigger than we are willing to read whole.</summary>
    TooLarge,

    /// <summary>A cloud placeholder whose content is not on this machine. Reading it would
    /// trigger a download, which is never something a preview may do on its own.</summary>
    NotDownloaded,

    /// <summary>Nothing could produce a preview. Produced at runtime, when the shell declines,
    /// rather than by the classifier — which cannot know what handlers are installed.</summary>
    NoPreview,
}

/// <summary>What the classifier is given: everything decidable without touching the file.</summary>
public readonly record struct PreviewTarget(
    string Name,
    long SizeBytes,
    FileAttributes Attributes,
    bool IsDirectory);

/// <summary>The classifier's answer: how to read the file, or why not to.</summary>
/// <param name="Kind">What to show. <see cref="PreviewKind.None"/> when <paramref name="Refusal"/> is set.</param>
/// <param name="Refusal">Why not, or <see cref="PreviewRefusal.None"/>.</param>
/// <param name="ByteBudget">The most that may be read from the file. Zero when nothing is read
/// from it directly (media and documents go through the shell, which streams its own way).</param>
/// <param name="Language">Which syntax table applies, for <see cref="PreviewKind.Text"/>.</param>
/// <param name="Mode">What the user asked for. Carried on the plan rather than read from the view
/// model by the background build, so a plan cannot be executed under a mode it was not made for.</param>
public sealed record PreviewRequest(
    PreviewKind Kind,
    PreviewRefusal Refusal,
    long ByteBudget,
    SyntaxLanguage Language,
    PreviewMode Mode = PreviewMode.Auto)
{
    public bool IsRefused => Refusal != PreviewRefusal.None;
}
