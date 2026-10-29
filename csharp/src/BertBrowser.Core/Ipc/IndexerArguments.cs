namespace BertBrowser.Core.Ipc;

/// <summary>What the index helper was started to do.</summary>
public enum IndexerCommand
{
    /// <summary>Index, and serve apps as they come and go. The ordinary case.</summary>
    Run,

    /// <summary>Register the sign-in task, then exit.</summary>
    RegisterAutoStart,

    /// <summary>Remove the sign-in task, then exit.</summary>
    UnregisterAutoStart,
}

/// <summary>
/// The elevated indexer's command line: what to do, and where the database is.
/// </summary>
/// <remarks>
/// <para>
/// Pure, like <c>CommandLine</c> is, so every rule here is testable without a process to launch.
/// </para>
/// <para>
/// <b>It is no longer told which pipe to call back on, and that is the point.</b> The name is
/// derived from this process's own token (see <see cref="IndexEndpoint"/>), so the one argument
/// that used to let a caller aim the elevated process at an endpoint of their choosing does not
/// exist any more — a stronger property than validating it was. The parent process id went with it:
/// the helper outlives whoever launched it, so there is no parent to expect or to watch.
/// </para>
/// <para>
/// <b>The data directory is still passed and still checked.</b> It is genuinely not derivable here:
/// it honours an override the UI harness relies on so a scripted run cannot touch the user's real
/// index, and working it out on this side would make that isolation depend on environment
/// inheritance across an elevation boundary. So it arrives from a process this one does not control
/// and is checked the way any other inbound path is.
/// </para>
/// </remarks>
public sealed record IndexerArguments(IndexerCommand Command, string DataDirectory)
{
    /// <summary>
    /// Parses, or explains why not. The error is for a log — nobody types this command line.
    /// </summary>
    public static bool TryParse(IReadOnlyList<string> args, out IndexerArguments result, out string error)
    {
        result = null!;
        error = "";

        string? dataDir = null;
        IndexerCommand? command = null;

        for (var i = 0; i < args.Count; i++)
        {
            var value = i + 1 < args.Count ? args[i + 1] : null;
            switch (args[i])
            {
                case "--data-dir" when value is not null:
                    dataDir = value;
                    i++;
                    break;

                case "--register-autostart":
                case "--unregister-autostart":
                    if (command is not null)
                    {
                        error = "Only one of --register-autostart and --unregister-autostart may be given.";
                        return false;
                    }
                    command = args[i] == "--register-autostart"
                        ? IndexerCommand.RegisterAutoStart
                        : IndexerCommand.UnregisterAutoStart;
                    break;

                default:
                    // An unrecognised option is an error, never a positional value — the same rule
                    // the user-facing command line follows, and for the same reason. --pipe and
                    // --parent-pid land here now, which is what makes their removal real rather
                    // than a validator nothing calls.
                    error = $"Unrecognised argument: {args[i]}";
                    return false;
            }
        }

        if (dataDir is null)
        {
            error = "Usage: BertBrowser.Indexer [--register-autostart|--unregister-autostart] --data-dir <path>";
            return false;
        }

        if (!Cli.NavigationRequest.IsAcceptablePath(dataDir))
        {
            error = "The data directory is not an acceptable absolute path.";
            return false;
        }

        result = new IndexerArguments(command ?? IndexerCommand.Run, dataDir);
        return true;
    }
}
