using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Rename;

namespace BertBrowser.Core.Services.NewItem;

/// <summary>The filesystem questions <see cref="NewItemPlanner"/> asks. Abstracted so the rules
/// that stop a create landing on something can be unit-tested without a real disk.</summary>
public interface INewItemProbe
{
    bool DirectoryExists(string path);

    bool FileExists(string path);
}

/// <summary>Real-filesystem <see cref="INewItemProbe"/>.</summary>
public sealed class FileSystemNewItemProbe : INewItemProbe
{
    public bool DirectoryExists(string path) => Directory.Exists(path);

    public bool FileExists(string path) => File.Exists(path);
}

/// <summary>
/// Works out what creating a folder or file would produce, without touching anything.
/// </summary>
/// <remarks>
/// <para>
/// This exists for one reason: the dialog asks on every keystroke, so the rule it previews has to be
/// the rule the create obeys. A refusal the user can see while the name is still editable is worth
/// far more than an error afterwards.
/// </para>
/// <para>
/// <see cref="Delete.ProtectedLocations"/> is deliberately <em>not</em> consulted. That list guards
/// a handful of folders against being <em>deleted</em>, exact-match only; creating inside them is
/// the ordinary thing this app is for — a new file in the profile root is not a mistake, and
/// <c>C:\Windows</c> is refused by its ACL now the app is <c>asInvoker</c>. Wiring it in here would
/// refuse legitimate work and buy nothing.
/// </para>
/// </remarks>
public sealed class NewItemPlanner
{
    private readonly INewItemProbe _probe;

    public NewItemPlanner(INewItemProbe probe) => _probe = probe;

    public NewItemPlanner() : this(new FileSystemNewItemProbe())
    {
    }

    /// <summary>What creating <paramref name="name"/> in <paramref name="directory"/> would
    /// produce. Never throws: an unusable path is a refusal like any other.</summary>
    public NewItemPlan Plan(
        string directory, string name, NewItemKind kind, string? templatePath = null)
    {
        var cleaned = NewItemPattern.Clean(name);
        var plan = new NewItemPlan(directory, cleaned, kind, templatePath, null);

        if (!IsUsable(directory) || !_probe.DirectoryExists(directory))
        {
            return plan with
            {
                Rejected = new RejectedNewItem(
                    NewItemRejection.ParentMissing,
                    "The folder to create this in is no longer there."),
            };
        }

        // The pattern's message, not one of ours: a name Windows refuses should be explained the
        // same way here as it is in the rename dialog.
        if (NewItemPattern.Validate(cleaned) is { } problem)
            return plan with { Rejected = new RejectedNewItem(NewItemRejection.InvalidName, problem) };

        if (templatePath is { Length: > 0 } template && !_probe.FileExists(template))
        {
            return plan with
            {
                Rejected = new RejectedNewItem(
                    NewItemRejection.TemplateMissing,
                    $"The template for this file type is missing: {template}"),
            };
        }

        var target = plan.TargetPath;
        if (!IsUsable(target))
        {
            return plan with
            {
                Rejected = new RejectedNewItem(
                    NewItemRejection.InvalidName, "That name can't be used here."),
            };
        }

        // Files and folders share one namespace, so either kind in the way is a collision.
        if (_probe.DirectoryExists(target) || _probe.FileExists(target))
        {
            return plan with
            {
                Rejected = new RejectedNewItem(
                    NewItemRejection.NameTaken, $"'{cleaned}' already exists in this folder."),
            };
        }

        return plan;
    }

    /// <summary>The name the dialog opens with: <paramref name="baseName"/>, stepped aside to
    /// "(2)" and so on if that is already taken.</summary>
    public string SuggestName(
        string directory, string baseName, NewItemKind kind, string extension = "") =>
        NewItemPattern.SuggestName(_probe, directory, baseName, kind, extension);

    /// <summary>Whether a path can be canonicalized at all. The other planners swallow exactly
    /// these three for the same reason: an unusable path is the user's problem to see, not an
    /// exception out of a method the dialog calls on every keystroke.</summary>
    private static bool IsUsable(string path)
    {
        try
        {
            PathKey.Canonicalize(path);
            return true;
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException
                                       or PathTooLongException)
        {
            return false;
        }
    }
}
