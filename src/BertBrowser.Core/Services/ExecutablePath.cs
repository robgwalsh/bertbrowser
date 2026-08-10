namespace BertBrowser.Core.Services;

/// <summary>
/// Resolves a program name — <c>wt.exe</c>, <c>code</c>, or a path the user typed into a custom
/// command — to the absolute path of a file that exists, the way Windows searches <c>PATH</c>.
/// </summary>
/// <remarks>
/// <para>
/// This exists because launches no longer go through <c>Process.Start</c>. The shell route that
/// de-elevates them reports nothing back: no process handle, no exit code, and no signal that the
/// target was even found. "Open in Terminal" used to discover that Windows Terminal was not
/// installed by letting <c>Process.Start</c> <b>throw</b> and falling through to PowerShell in the
/// <c>catch</c> — with the throw gone, that fallback would silently do nothing at all. Resolving
/// first turns "is it installed?" back into a question with an answer: a null return is the
/// fall-through signal.
/// </para>
/// <para>
/// It is also the safer order of operations. Handing a bare <c>code</c> to the shell lets an
/// <b>elevated</b> process resolve a name through a search path that may contain a directory some
/// lesser account can write to; resolving to an absolute path first, here, decides what runs.
/// </para>
/// <para>
/// Pure, and takes its environment and its "does this exist" probe as arguments, so the search
/// order is unit-tested rather than inferred from whatever is installed on the machine running the
/// tests.
/// </para>
/// </remarks>
public static class ExecutablePath
{
    /// <summary>What Windows uses when <c>PATHEXT</c> is unset.</summary>
    public const string DefaultPathExt = ".COM;.EXE;.BAT;.CMD";

    /// <summary>
    /// The absolute path <paramref name="program"/> resolves to, or null if nothing matches.
    /// </summary>
    /// <param name="program">A bare name (<c>code</c>), a name with an extension (<c>wt.exe</c>),
    /// or a full path.</param>
    /// <param name="pathVariable">The <c>PATH</c> value to search, <c>;</c>-separated.</param>
    /// <param name="pathExt">The <c>PATHEXT</c> value — extensions to try on a name that has
    /// none. Falls back to <see cref="DefaultPathExt"/> when blank.</param>
    /// <param name="exists">Probes for a file. Injected so the search order is testable.</param>
    public static string? Resolve(string? program, string? pathVariable, string? pathExt, Func<string, bool> exists)
    {
        ArgumentNullException.ThrowIfNull(exists);

        if (string.IsNullOrWhiteSpace(program)) return null;

        program = program.Trim().Trim('"');
        if (program.Length == 0) return null;

        var extensions = ParseExtensions(pathExt);

        // Something that already names a location is not a PATH lookup — take it or leave it, but
        // never fall back to searching, or "C:\gone\tool.exe" could start a different tool.
        if (NamesALocation(program))
        {
            // ...and it has to say *which* location. A relative path resolves against the current
            // directory, which for this process is wherever it happened to be started — an
            // elevated process must not let that decide what runs.
            if (!Path.IsPathFullyQualified(program)) return null;
            return FirstThatExists(program, extensions, exists);
        }

        foreach (var directory in ParseSearchPath(pathVariable))
        {
            string candidate;
            try
            {
                candidate = Path.Combine(directory, program);
            }
            catch (ArgumentException)
            {
                // A PATH entry with invalid characters in it — skip that entry, not the search.
                continue;
            }

            if (FirstThatExists(candidate, extensions, exists) is { } found) return found;
        }

        return null;
    }

    /// <summary>
    /// A name carrying its own extension is tried only as written; a bare one is tried against each
    /// <c>PATHEXT</c> in turn, which is what finds <c>code.cmd</c> for <c>code</c>. Deliberately not
    /// both: appending extensions to a name that has one would let <c>wt.exe.bat</c> answer for
    /// <c>wt.exe</c>.
    /// </summary>
    private static string? FirstThatExists(string candidate, IReadOnlyList<string> extensions, Func<string, bool> exists)
    {
        if (Path.HasExtension(candidate))
            return Safely(candidate, exists) ? Absolute(candidate) : null;

        foreach (var extension in extensions)
        {
            var withExtension = candidate + extension;
            if (Safely(withExtension, exists)) return Absolute(withExtension);
        }

        return null;
    }

    /// <summary>The probe is caller-supplied and reaches the filesystem; a path it dislikes costs
    /// that one candidate rather than the whole resolution.</summary>
    private static bool Safely(string candidate, Func<string, bool> exists)
    {
        try
        {
            return exists(candidate);
        }
        catch (Exception ex) when (ex is ArgumentException or IOException or UnauthorizedAccessException or NotSupportedException)
        {
            return false;
        }
    }

    private static string Absolute(string path)
    {
        try
        {
            return Path.GetFullPath(path);
        }
        catch (Exception ex) when (ex is ArgumentException or IOException or NotSupportedException)
        {
            return path;
        }
    }

    private static bool NamesALocation(string program) =>
        program.Contains('\\', StringComparison.Ordinal) ||
        program.Contains('/', StringComparison.Ordinal) ||
        Path.IsPathRooted(program);

    private static IReadOnlyList<string> ParseExtensions(string? pathExt)
    {
        var source = string.IsNullOrWhiteSpace(pathExt) ? DefaultPathExt : pathExt;
        var extensions = new List<string>();

        foreach (var raw in source.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        {
            var extension = raw.Trim('"');
            if (extension.Length == 0) continue;
            extensions.Add(extension[0] == '.' ? extension : "." + extension);
        }

        return extensions.Count == 0 ? ParseExtensions(DefaultPathExt) : extensions;
    }

    private static IEnumerable<string> ParseSearchPath(string? pathVariable)
    {
        if (string.IsNullOrWhiteSpace(pathVariable)) yield break;

        foreach (var raw in pathVariable.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        {
            // PATH entries are commonly quoted when they contain spaces.
            var directory = raw.Trim('"').Trim();
            if (directory.Length > 0) yield return directory;
        }
    }
}
