using System.Text;

namespace BertBrowser.Harness;

/// <summary>
/// The throwaway tree a run browses, and the fence around everything that writes.
/// </summary>
/// <remarks>
/// BertBrowser's job is moving, renaming and deleting the user's files, and the harness drives the
/// real executors — not stubs. So a script's targets are checked against this root before anything
/// is planned, and <c>--allow-outside</c> is the one deliberate way past it.
/// </remarks>
internal sealed class Sandbox(HarnessOptions options)
{
    public string Root { get; } = options.SandboxDir;

    /// <summary>Resolves a script path: absolute as given, otherwise relative to the sandbox.</summary>
    public string Resolve(string path) =>
        Path.IsPathRooted(path) ? Path.GetFullPath(path) : Path.GetFullPath(Path.Combine(Root, path));

    /// <summary>
    /// Refuses a path the run has no business writing to.
    /// </summary>
    /// <remarks>
    /// Compared through <see cref="Path.GetFullPath"/> so <c>..</c> cannot walk out, and with a
    /// trailing separator so a sibling directory whose name merely starts with the sandbox's does
    /// not pass.
    /// </remarks>
    public string RequireInside(string path, string verb)
    {
        var full = Resolve(path);
        if (options.AllowOutside) return full;

        var fenced = Root.TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
        if (!full.StartsWith(fenced, StringComparison.OrdinalIgnoreCase) &&
            !full.Equals(Root.TrimEnd(Path.DirectorySeparatorChar), StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException(
                $"{verb} would touch '{full}', which is outside the run's sandbox ({Root}). " +
                "Build fixtures with 'tree'/'mkdir'/'write', or pass --allow-outside if you mean it.");
        }

        return full;
    }

    /// <summary>
    /// Lays down a small tree worth browsing: nested folders, a hidden entry, files of several
    /// types and sizes so sorting, the size column and the type column all have something to show.
    /// </summary>
    /// <remarks>
    /// Deterministic on purpose — same names, same byte counts, same modified times every run — so
    /// two captures of the same script differ only where the change under test made them differ.
    /// </remarks>
    public string Populate(string relative)
    {
        var root = Resolve(relative);
        Directory.CreateDirectory(root);

        Write(Path.Combine(root, "notes.txt"), 240);
        Write(Path.Combine(root, "report.md"), 1_100);
        Write(Path.Combine(root, "budget.xlsx"), 18_400);
        Write(Path.Combine(root, "photo.jpg"), 96_000);
        Write(Path.Combine(root, "archive.zip"), 512_000);

        var documents = Path.Combine(root, "Documents");
        Directory.CreateDirectory(documents);
        Write(Path.Combine(documents, "letter.txt"), 800);
        Write(Path.Combine(documents, "report.md"), 2_400); // a duplicate name, for search results
        Write(Path.Combine(documents, "draft.md"), 640);

        var nested = Path.Combine(documents, "Archive");
        Directory.CreateDirectory(nested);
        Write(Path.Combine(nested, "report.md"), 300); // three levels down, so search must recurse
        Write(Path.Combine(nested, "old.txt"), 120);

        var pictures = Path.Combine(root, "Pictures");
        Directory.CreateDirectory(pictures);
        Write(Path.Combine(pictures, "holiday.jpg"), 240_000);
        Write(Path.Combine(pictures, "diagram.png"), 44_000);

        var empty = Path.Combine(root, "Empty");
        Directory.CreateDirectory(empty);

        var hiddenFile = Path.Combine(root, ".hidden-notes.txt");
        Write(hiddenFile, 64);
        File.SetAttributes(hiddenFile, File.GetAttributes(hiddenFile) | FileAttributes.Hidden);

        var hiddenDir = Path.Combine(root, "HiddenFolder");
        Directory.CreateDirectory(hiddenDir);
        Write(Path.Combine(hiddenDir, "secret.txt"), 32);
        File.SetAttributes(hiddenDir, File.GetAttributes(hiddenDir) | FileAttributes.Hidden);

        Stamp(root);
        return root;
    }

    /// <summary>Writes a file of a given size, its contents derived from its own name so a copy
    /// can be told from a coincidence.</summary>
    /// <remarks>
    /// Clears hidden and read-only first, because <see cref="File.WriteAllText(string, string)"/>
    /// refuses to open an existing hidden file — which is exactly what re-running a script into an
    /// output directory that already has a tree in it would hit.
    /// </remarks>
    public static void Write(string path, int bytes)
    {
        if (Path.GetDirectoryName(path) is { Length: > 0 } directory)
            Directory.CreateDirectory(directory);

        if (File.Exists(path))
            File.SetAttributes(path, File.GetAttributes(path) & ~(FileAttributes.Hidden | FileAttributes.ReadOnly));

        var seed = Path.GetFileName(path) + "\n";
        var text = new StringBuilder(bytes + seed.Length);
        while (text.Length < bytes) text.Append(seed);

        File.WriteAllText(path, text.ToString(0, Math.Max(bytes, seed.Length)));
    }

    /// <summary>
    /// Gives everything the same modified time.
    /// </summary>
    /// <remarks>
    /// The list shows a Modified column, so without this every capture of the same script differs
    /// in a way that has nothing to do with what changed. A fixed date in the past also keeps the
    /// "Modified" sort meaningful rather than a tie.
    /// </remarks>
    public static void Stamp(string root)
    {
        var when = new DateTime(2024, 3, 14, 9, 26, 53, DateTimeKind.Local);

        foreach (var file in Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories))
            File.SetLastWriteTime(file, when);

        foreach (var directory in Directory.EnumerateDirectories(root, "*", SearchOption.AllDirectories))
            Directory.SetLastWriteTime(directory, when);

        Directory.SetLastWriteTime(root, when);
    }
}
