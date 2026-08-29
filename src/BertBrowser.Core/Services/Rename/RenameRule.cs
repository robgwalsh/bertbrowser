using System.Text.Json.Serialization;

namespace BertBrowser.Core.Services.Rename;

/// <summary>How the scoped part of a name is re-cased.</summary>
public enum RenameCase
{
    AsIs,
    Lower,
    Upper,
    Title,
    Sentence,
}

/// <summary>
/// Which part of the <em>existing</em> name find/replace and the case transform act on.
/// </summary>
/// <remarks>
/// Never the finished name. A template's own literals — and <c>{parent}</c> — are not in scope,
/// so "{parent} - {name}" with a space-to-underscore replace still produces a name with spaces
/// in it. Cleaning up the finished name is a different operation and deliberately not this one;
/// the value is named for the source so the UI cannot promise otherwise.
/// </remarks>
public enum RenameScope
{
    /// <summary>The name without its extension. A folder is all stem.</summary>
    Stem,

    /// <summary>The extension, leading dot included. Empty for a folder.</summary>
    Extension,

    /// <summary>Stem and extension together.</summary>
    WholeName,
}

/// <summary>
/// Everything the rename dialog can ask for: a name template, a find/replace, a case transform
/// and a counter. Pure data — <see cref="RenamePattern.Apply(IReadOnlyList{RenameSource}, RenameRule)"/>
/// is what turns it into names, and the dialog previews every keystroke through that same
/// function so a preview cannot drift from the result.
/// </summary>
public sealed record RenameRule
{
    [JsonConstructor]
    public RenameRule(
        string Template,
        string Find = "",
        string Replace = "",
        bool UseRegex = false,
        bool MatchCase = false,
        RenameScope Scope = RenameScope.Stem,
        RenameCase Case = RenameCase.AsIs,
        int CounterStart = 1,
        int CounterStep = 1,
        bool IsLiteral = false)
    {
        this.Template = Template;
        this.Find = Find;
        this.Replace = Replace;
        this.UseRegex = UseRegex;
        this.MatchCase = MatchCase;
        this.Scope = Scope;
        this.Case = Case;
        this.CounterStart = CounterStart;
        this.CounterStep = CounterStep;
        this.IsLiteral = IsLiteral;
    }

    public string Template { get; init; }

    public string Find { get; init; }

    public string Replace { get; init; }

    public bool UseRegex { get; init; }

    public bool MatchCase { get; init; }

    /// <remarks>
    /// Written as a name, not a number. <c>AppSettings.Save</c> serialises with bare options, so
    /// without this these land in settings.json as 2 and 3 — against the hand-editable position
    /// <c>TileAspectRatio</c> takes, and reordering the enum later would silently reinterpret
    /// whatever was already saved. The attribute goes here rather than on the global options so
    /// nothing else in that file changes shape.
    /// </remarks>
    [JsonConverter(typeof(JsonStringEnumConverter))]
    public RenameScope Scope { get; init; }

    /// <inheritdoc cref="Scope"/>
    [JsonConverter(typeof(JsonStringEnumConverter))]
    public RenameCase Case { get; init; }

    public int CounterStart { get; init; }

    public int CounterStep { get; init; }

    /// <summary>
    /// True for the rule the plain F2 box has always produced: the template is taken literally,
    /// braces and all, and no token is expanded.
    /// </summary>
    /// <remarks>
    /// This is what keeps the simple path byte-identical to what it was. '{' and '}' are legal
    /// Windows filename characters — <c>{6B99A0C1-…}.tmp</c> and <c>{id}.tsx</c> are names a file
    /// browser is routinely asked to produce — so a box that expanded tokens everywhere would
    /// refuse, or silently rewrite, a name it accepts today. Tokens are offered only by the
    /// expanded panel, where the list of them is on screen beside the box.
    /// </remarks>
    public bool IsLiteral { get; init; }

    /// <summary>The literal, token-free rule the plain rename box produces.</summary>
    public static RenameRule Simple(string template) => new(template, IsLiteral: true);

    /// <summary>What the expanded panel starts from before anything is typed.</summary>
    public static RenameRule Default { get; } = new("{name}");
}
