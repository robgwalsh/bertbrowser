namespace BertBrowser.Core.Services.ShellMenu;

public enum ShellExtensionKind
{
    /// <summary>A COM handler, asked to build its own items.</summary>
    Handler,

    /// <summary>A static registry verb: a name and a command line.</summary>
    Verb,
}

/// <summary>
/// One row of the Settings checklist and one contributor to a menu: a handler or a verb, merged
/// across every key family it is registered under, so 7-Zip is one row and not four.
/// </summary>
/// <param name="Id">What the hidden list stores. Stable across sessions and machines:
/// <c>clsid:{guid}</c> or <c>verb:name</c>, lowercase.</param>
/// <param name="Verbs">Every registration of a verb, one per family — invoking on the background
/// uses the <c>Directory\Background</c> one, whose command line differs.</param>
public sealed record ShellExtension(
    string Id,
    string Name,
    ShellExtensionKind Kind,
    Guid? Clsid,
    IReadOnlyList<ShellVerbRegistration> Verbs,
    IReadOnlyList<string> Families)
{
    public static string HandlerId(Guid clsid) => "clsid:" + clsid.ToString("B").ToLowerInvariant();

    public static string VerbId(string verb) => "verb:" + verb.ToLowerInvariant();
}
