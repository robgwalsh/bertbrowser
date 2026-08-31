namespace BertBrowser.Core.Services.NewItem;

/// <summary>
/// Carries out a <see cref="NewItemPlan"/>.
/// </summary>
/// <remarks>
/// <para>
/// Nothing is ever overwritten, and — the part worth stating separately — nothing existing is ever
/// <em>adopted</em>. <see cref="Directory.CreateDirectory(string)"/> succeeds silently on a folder
/// that is already there, so a create that raced something else would hand the user someone else's
/// folder and report success. Existence is therefore checked immediately before the call rather
/// than trusted from the plan, which was built while the dialog sat open.
/// </para>
/// <para>
/// Files need no such care and do not get it: <see cref="FileMode.CreateNew"/> and
/// <see cref="File.Copy(string, string, bool)"/> with <c>overwrite: false</c> both throw if the path
/// is taken, which closes the window a check-then-create leaves open.
/// </para>
/// </remarks>
public sealed class NewItemExecutor
{
    public NewItemOutcome Execute(NewItemPlan plan)
    {
        if (!plan.HasWork) return NewItemOutcome.Empty;

        var target = plan.TargetPath;
        try
        {
            if (plan.Kind == NewItemKind.Folder)
            {
                // Re-checked against live disk, not taken from the plan — see the remarks above.
                if (Directory.Exists(target) || File.Exists(target)) return AlreadyThere(plan);
                Directory.CreateDirectory(target);
            }
            else if (plan.TemplatePath is { Length: > 0 } template)
            {
                if (!File.Exists(template))
                {
                    return new NewItemOutcome(null, new FailedNewItem(
                        $"The template for this file type is missing: {template}"));
                }

                File.Copy(template, target, overwrite: false);
                ClearInheritedAttributes(target);
            }
            else
            {
                // Creates and closes; the file is meant to be empty.
                using var stream = new FileStream(
                    target, FileMode.CreateNew, FileAccess.Write, FileShare.None);
            }

            return new NewItemOutcome(target, null);
        }
        catch (IOException) when (Directory.Exists(target) || File.Exists(target))
        {
            // The atomic creates report a taken path as an IOException; say what actually happened
            // rather than passing on "the file already exists" from a layer down.
            return AlreadyThere(plan);
        }
        catch (Exception ex) when (IsFilesystemFailure(ex))
        {
            return new NewItemOutcome(null, new FailedNewItem(
                $"Could not create '{plan.Name}' — {ex.Message}", AccessDenied.Caused(ex)));
        }
    }

    private static NewItemOutcome AlreadyThere(NewItemPlan plan) =>
        new(null, new FailedNewItem($"'{plan.Name}' already exists in this folder."));

    /// <summary>A template kept under <c>%APPDATA%\Microsoft\Windows\Templates</c> is often marked
    /// Hidden or ReadOnly; the file the user asked for should be neither. Best-effort — a copy that
    /// was made is not a failure just because its attributes would not come off.</summary>
    private static void ClearInheritedAttributes(string path)
    {
        try
        {
            var attributes = File.GetAttributes(path);
            var wanted = attributes & ~(FileAttributes.Hidden | FileAttributes.ReadOnly
                | FileAttributes.System);
            if (wanted != attributes) File.SetAttributes(path, wanted);
        }
        catch (Exception ex) when (IsFilesystemFailure(ex))
        {
        }
    }

    /// <summary>Errors that mean "this failed" rather than "the program is broken".</summary>
    private static bool IsFilesystemFailure(Exception ex) =>
        ex is IOException or UnauthorizedAccessException or ArgumentException
            or NotSupportedException or System.Security.SecurityException;
}
