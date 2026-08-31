namespace BertBrowser.Core.Ipc;

/// <summary>
/// The elevated file-operation helper's command line: which pipe to call back on, which process
/// started it, and whose SID that process is expected to be running as.
/// </summary>
/// <remarks>
/// <para>
/// <b>There is deliberately no <c>--data-dir</c>.</b> The index helper needs one because it writes
/// to the database; this one never opens it. Every argument that does not exist is one an attacker
/// cannot choose, and the whole surface here is a pipe name, a number and a SID — none of which
/// names a file.
/// </para>
/// <para>
/// <b><see cref="UserSid"/> is a check, never a source of truth.</b> UAC gives the same user a
/// different token, not a different user, so the helper's own
/// <c>WindowsIdentity.GetCurrent().User</c> already <em>is</em> the calling user's SID — which is
/// what it uses when it has to grant that user access to a staging folder it creates. This argument
/// exists only so a mismatch can be refused, which covers the over-the-shoulder credential prompt
/// where the elevating identity belongs to somebody else. Trusting it instead would be taking an
/// attacker-chosen SID and writing it into an ACL.
/// </para>
/// </remarks>
public sealed record ElevatorArguments(string PipeName, int ParentProcessId, string UserSid)
{
    public const int MaxPipeNameLength = 256;

    /// <summary>The endpoint prefix this helper will talk to, and nothing else. Separate from the
    /// index helper's so neither can ever be handed the other's pipe.</summary>
    public const string PipePrefix = "BertBrowser.Elevate.";

    private const string Usage =
        "Usage: BertBrowser.Elevator --pipe <name> --parent-pid <id> --user-sid <sid>";

    /// <summary>Parses, or explains. An unrecognised token is an error and never a positional
    /// value — the same rule <c>IndexerArguments</c> and <c>CommandLine</c> follow, and for the same
    /// reason: a mistyped flag silently becoming data is worse than a message.</summary>
    public static bool TryParse(
        IReadOnlyList<string> args, out ElevatorArguments result, out string error)
    {
        result = new ElevatorArguments("", 0, "");
        error = "";

        string? pipe = null;
        string? sid = null;
        var parentPid = 0;

        for (var i = 0; i < args.Count; i++)
        {
            switch (args[i])
            {
                case "--pipe" when i + 1 < args.Count:
                    pipe = args[++i];
                    break;

                case "--parent-pid" when i + 1 < args.Count:
                    if (!int.TryParse(args[++i], out parentPid) || parentPid <= 0)
                    {
                        error = "--parent-pid must be a process id.";
                        return false;
                    }
                    break;

                case "--user-sid" when i + 1 < args.Count:
                    sid = args[++i];
                    break;

                default:
                    error = $"'{args[i]}' is not a recognised argument. {Usage}";
                    return false;
            }
        }

        if (!IsAcceptablePipeName(pipe))
        {
            error = $"--pipe is missing or not an acceptable endpoint. {Usage}";
            return false;
        }

        if (parentPid <= 0)
        {
            error = $"--parent-pid is missing. {Usage}";
            return false;
        }

        if (!IsAcceptableSid(sid))
        {
            error = $"--user-sid is missing or malformed. {Usage}";
            return false;
        }

        result = new ElevatorArguments(pipe!, parentPid, sid!);
        return true;
    }

    /// <summary>An endpoint this helper is willing to connect to: bounded, free of the characters a
    /// pipe name cannot hold, and carrying this helper's own prefix.</summary>
    public static bool IsAcceptablePipeName(string? candidate)
    {
        if (string.IsNullOrWhiteSpace(candidate)) return false;
        if (candidate.Length > MaxPipeNameLength) return false;
        if (candidate.Any(char.IsControl)) return false;
        if (candidate.IndexOfAny(['\\', '/', '*', '?', '|', '<', '>', '"', ':']) >= 0) return false;
        return candidate.StartsWith(PipePrefix, StringComparison.Ordinal);
    }

    /// <summary>The textual form of a SID: <c>S-1-…</c>, digits and hyphens only. Not a full parse —
    /// the helper compares it against its own identity, which is the actual check.</summary>
    public static bool IsAcceptableSid(string? candidate)
    {
        if (string.IsNullOrWhiteSpace(candidate)) return false;
        if (candidate.Length is < 3 or > 256) return false;
        if (!candidate.StartsWith("S-", StringComparison.Ordinal)) return false;
        return candidate.Skip(2).All(c => char.IsAsciiDigit(c) || c == '-');
    }
}
