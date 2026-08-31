namespace BertBrowser.Core.Services.NewItem;

/// <summary>What is being created.</summary>
public enum NewItemKind
{
    Folder,

    File,
}

/// <summary>Why the planner refused to create the item. All four are worth showing in the dialog
/// while the user can still do something about them, which is the point of planning at all: nothing
/// is written until a plan comes back clean.</summary>
public enum NewItemRejection
{
    /// <summary>Not a legal Windows file name — see <see cref="NewItemPattern.Validate"/>.</summary>
    InvalidName,

    /// <summary>Something is already there. Creating is never allowed to overwrite, and for a
    /// folder it is not even allowed to quietly adopt what is there already.</summary>
    NameTaken,

    /// <summary>The folder being created in has gone since the menu was opened.</summary>
    ParentMissing,

    /// <summary>The chosen file type points at a template file that is no longer there. Refused up
    /// front rather than producing an empty file the user did not ask for.</summary>
    TemplateMissing,
}

/// <param name="Reason">Why.</param>
/// <param name="Message">User-facing explanation, shown under the name box as it is typed.</param>
public sealed record RejectedNewItem(NewItemRejection Reason, string Message);

/// <summary>The validated answer to "what would creating this produce?". Unlike a rename or a
/// delete there is only ever one item, so the plan carries one rejection rather than a list.</summary>
/// <param name="Directory">The folder the item goes in.</param>
/// <param name="Name">The item's name, extension included.</param>
/// <param name="Kind">Folder or file.</param>
/// <param name="TemplatePath">A file to copy the new file's contents from, or null for an empty
/// one. Always null for a folder.</param>
/// <param name="Rejected">Why this cannot be created, or null when it can.</param>
public sealed record NewItemPlan(
    string Directory,
    string Name,
    NewItemKind Kind,
    string? TemplatePath,
    RejectedNewItem? Rejected)
{
    /// <summary>True when carrying the plan out would create something.</summary>
    public bool HasWork => Rejected is null && Name.Length > 0;

    public string TargetPath => System.IO.Path.Combine(Directory, Name);

    public static NewItemPlan Empty { get; } =
        new("", "", NewItemKind.Folder, null, null);
}

/// <param name="Message">The failure, phrased for a message dialog.</param>
/// <param name="AccessDenied">Windows refused permission. The only failure an administrator token
/// could fix.</param>
public sealed record FailedNewItem(string Message, bool AccessDenied = false);

/// <summary>What actually happened on disk. There is deliberately no undo record: creating is
/// additive, exactly as copying is, and the item it makes is empty. Ctrl+Z is left pointing at
/// whatever move, rename or delete came before.</summary>
public sealed record NewItemOutcome(string? CreatedPath, FailedNewItem? Failed)
{
    public bool Created => CreatedPath is not null;

    public static NewItemOutcome Empty { get; } = new(null, null);
}
