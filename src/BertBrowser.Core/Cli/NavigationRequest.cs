using System.Text;

namespace BertBrowser.Core.Cli;

/// <summary>
/// The wire format a second instance uses to hand its command line to the one already running, and
/// the single rule deciding whether a path may be acted on at all.
/// </summary>
/// <remarks>
/// <para>
/// <b><see cref="IsAcceptablePath"/> is the rule worth auditing.</b> It is used by the command line
/// <em>and</em> by the pipe listener, so there is one place to get right rather than two — the same
/// discipline <c>ThemeId.IsSafe</c> gets, and for the same reason: this process is elevated, and a
/// path arriving over IPC is untrusted input.
/// </para>
/// <para>
/// What it permits is narrow on purpose: an absolute local or UNC path, and nothing else. It cannot
/// become an argument to a launch, a filename that gets written, or a database key — the only thing
/// on the other end of it is a directory listing.
/// </para>
/// <para>
/// The format is one line per request, tab-separated. Tab is a safe separator precisely because
/// <see cref="IsAcceptablePath"/> rejects control characters; a filename containing one is
/// pathological and refusing it is the right trade for a format that cannot be confused by its own
/// payload.
/// </para>
/// </remarks>
public static class NavigationRequest
{
    /// <summary>Comfortably past any real path, and short enough that a hostile peer cannot make the
    /// listener allocate.</summary>
    public const int MaxPathLength = 4096;

    /// <summary>Caps a whole line, so a peer cannot stream forever into a reader.</summary>
    public const int MaxLineLength = 64 * 1024;

    private const char Separator = '\t';
    private const string Prefix = "OPEN";

    /// <summary>
    /// Whether a path may be navigated to. Rejects anything relative (it would resolve against
    /// whatever directory this process happens to be in), Win32 device paths, wildcards, control
    /// characters, and anything implausibly long.
    /// </summary>
    public static bool IsAcceptablePath(string? candidate)
    {
        if (string.IsNullOrWhiteSpace(candidate)) return false;
        if (candidate.Length > MaxPathLength) return false;

        foreach (var c in candidate)
        {
            // Control characters cover the separator, embedded newlines that would forge a second
            // request, and the NUL that truncates a string on the way into Win32.
            if (char.IsControl(c)) return false;
            if (c is '*' or '?' or '|' or '<' or '>' or '"') return false;
        }

        // "\\.\PhysicalDrive0" and friends are not files. "\\?\" is the long-path escape, which
        // bypasses normalization — neither is something a navigate request has any business naming.
        if (candidate.StartsWith(@"\\.\", StringComparison.Ordinal) ||
            candidate.StartsWith(@"\\?\", StringComparison.Ordinal))
            return false;

        try
        {
            // Rooted, and rooted in a way that names a volume: "\foo" is technically rooted but
            // means "the current drive", which is exactly the ambiguity being refused here.
            if (!Path.IsPathFullyQualified(candidate)) return false;
        }
        catch (ArgumentException)
        {
            return false;
        }

        return true;
    }

    /// <summary>Renders a request as one line. Targets failing
    /// <see cref="IsAcceptablePath"/> are dropped rather than sent.</summary>
    public static string Format(CommandLineRequest request)
    {
        var line = new StringBuilder(Prefix);
        line.Append(Separator).Append(request.Mode);

        foreach (var target in request.Targets)
        {
            if (!IsAcceptablePath(target.Path)) continue;
            line.Append(Separator).Append(target.Select ? 'S' : '-').Append(target.Path);
        }

        return line.ToString();
    }

    /// <summary>
    /// Reads a line back. False for anything malformed — a wrong prefix, an unknown mode, an
    /// oversized line — and paths that fail <see cref="IsAcceptablePath"/> are dropped, so a
    /// request that arrives half-corrupt opens the parts that were sound rather than nothing.
    /// </summary>
    public static bool TryParse(string? line, out CommandLineRequest request)
    {
        request = CommandLineRequest.Empty;
        if (string.IsNullOrEmpty(line) || line.Length > MaxLineLength) return false;

        var parts = line.Split(Separator);
        if (parts.Length < 2) return false;
        if (!parts[0].Equals(Prefix, StringComparison.Ordinal)) return false;
        if (!Enum.TryParse<OpenIn>(parts[1], ignoreCase: false, out var mode)) return false;

        var targets = new List<OpenTarget>();
        for (var i = 2; i < parts.Length; i++)
        {
            var field = parts[i];
            if (field.Length < 2) continue;

            var select = field[0] switch { 'S' => true, '-' => false, _ => (bool?)null };
            if (select is not { } wantsSelect) continue;

            var path = field[1..];
            if (!IsAcceptablePath(path)) continue;

            targets.Add(new OpenTarget(path, wantsSelect));
        }

        request = new CommandLineRequest(targets, mode, []);
        return true;
    }
}
