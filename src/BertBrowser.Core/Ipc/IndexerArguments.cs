namespace BertBrowser.Core.Ipc;

/// <summary>
/// The elevated indexer's command line: which pipe to call back on, which process to expect at the
/// other end of it, and where the database is.
/// </summary>
/// <remarks>
/// <para>
/// Pure, like <c>CommandLine</c> is, so every rule here is testable without a process to launch.
/// </para>
/// <para>
/// <b>Nothing here is trusted.</b> These arguments arrive from a process this one does not
/// control, so the data directory is checked the same way any other inbound path is, and the pipe
/// name is restricted to the shape this app generates. The parent process id is what the helper
/// checks the pipe's real owner against — an argument saying "expect process 1234" is worth
/// nothing on its own, and everything once <c>GetNamedPipeServerProcessId</c> has to agree with it.
/// </para>
/// </remarks>
public sealed record IndexerArguments(string PipeName, int ParentProcessId, string DataDirectory)
{
    /// <summary>Pipe names this app generates: the prefix, a SID, and a hex nonce.</summary>
    public const int MaxPipeNameLength = 256;

    /// <summary>
    /// Parses, or explains why not. The error is for a log — nobody types this command line.
    /// </summary>
    public static bool TryParse(IReadOnlyList<string> args, out IndexerArguments result, out string error)
    {
        result = null!;
        error = "";

        string? pipe = null, dataDir = null;
        int? parentPid = null;

        for (var i = 0; i < args.Count; i++)
        {
            var value = i + 1 < args.Count ? args[i + 1] : null;
            switch (args[i])
            {
                case "--pipe" when value is not null:
                    pipe = value;
                    i++;
                    break;
                case "--parent-pid" when value is not null:
                    if (!int.TryParse(value, out var pid) || pid <= 0)
                    {
                        error = "--parent-pid must be a positive process id.";
                        return false;
                    }
                    parentPid = pid;
                    i++;
                    break;
                case "--data-dir" when value is not null:
                    dataDir = value;
                    i++;
                    break;
                default:
                    // An unrecognised option is an error, never a positional value — the same rule
                    // the user-facing command line follows, and for the same reason.
                    error = $"Unrecognised argument: {args[i]}";
                    return false;
            }
        }

        if (pipe is null || parentPid is null || dataDir is null)
        {
            error = "Usage: BertBrowser.Indexer --pipe <name> --parent-pid <id> --data-dir <path>";
            return false;
        }

        if (!IsAcceptablePipeName(pipe))
        {
            error = "The pipe name is not one this app generates.";
            return false;
        }

        if (!Cli.NavigationRequest.IsAcceptablePath(dataDir))
        {
            error = "The data directory is not an acceptable absolute path.";
            return false;
        }

        result = new IndexerArguments(pipe, parentPid.Value, dataDir);
        return true;
    }

    /// <summary>
    /// A pipe name this app would have produced: no path separators, no wildcards, no control
    /// characters, and bounded. It becomes a <c>\\.\pipe\</c> path, so a separator in it would
    /// name a different object than the one intended.
    /// </summary>
    public static bool IsAcceptablePipeName(string? candidate)
    {
        if (string.IsNullOrEmpty(candidate)) return false;
        if (candidate.Length > MaxPipeNameLength) return false;

        foreach (var c in candidate)
        {
            if (char.IsControl(c)) return false;
            if (c is '\\' or '/' or '*' or '?' or '|' or '<' or '>' or '"' or ':') return false;
        }

        return candidate.StartsWith("BertBrowser.Index.", StringComparison.Ordinal);
    }
}
