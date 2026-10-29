namespace BertBrowser.Core.Cli;

/// <summary>Where a request wants its folders opened.</summary>
public enum OpenIn
{
    /// <summary>Whatever the app would do anyway — a tab in the active pane.</summary>
    Default,

    /// <summary>Force a new tab, even for a folder already showing.</summary>
    NewTab,

    /// <summary>A pane of its own, beside the active one. This app splits panes rather than opening
    /// second windows, so that — not a new window — is what "somewhere alongside" means here.</summary>
    NewPane,
}

/// <param name="Path">The folder to show, or — with <paramref name="Select"/> — the item to
/// highlight inside its parent.</param>
/// <param name="Select">Explorer's <c>/select</c>: open the item's <em>parent</em> and highlight
/// the item, rather than trying to browse into it.</param>
public sealed record OpenTarget(string Path, bool Select);

/// <param name="Errors">Anything that could not be understood. Reported rather than guessed at —
/// an unrecognised option must never be silently treated as a path.</param>
public sealed record CommandLineRequest(
    IReadOnlyList<OpenTarget> Targets, OpenIn Mode, IReadOnlyList<string> Errors)
{
    public bool HasTargets => Targets.Count > 0;

    public static CommandLineRequest Empty { get; } = new([], OpenIn.Default, []);
}

/// <summary>
/// Parses BertBrowser's command line.
/// </summary>
/// <remarks>
/// <para>
/// Pure: it never touches the filesystem, so "does this folder exist?" stays with the caller and
/// every rule here is testable. It is shared by the process's own startup arguments and by the
/// single-instance pipe, so there is one grammar rather than two that drift.
/// </para>
/// <para>
/// It understands Explorer's <c>/select,&lt;path&gt;</c> convention, which arrives as either one
/// argv token or two depending on whether a space followed the comma.
/// </para>
/// </remarks>
public static class CommandLine
{
    private const string SelectPrefix = "/select,";

    public static CommandLineRequest Parse(IReadOnlyList<string> args)
    {
        if (args.Count == 0) return CommandLineRequest.Empty;

        var targets = new List<OpenTarget>();
        var errors = new List<string>();
        var mode = OpenIn.Default;
        var modeChosen = false;
        var selectNext = false;

        foreach (var raw in args)
        {
            if (string.IsNullOrWhiteSpace(raw)) continue;
            var arg = raw.Trim();

            // Explorer's convention. "/select,C:\dir\file.txt" is one token; "/select," followed by
            // the path is what it emits when a space follows the comma.
            if (arg.StartsWith(SelectPrefix, StringComparison.OrdinalIgnoreCase))
            {
                var rest = arg[SelectPrefix.Length..];
                if (rest.Length > 0) targets.Add(new OpenTarget(Repair(rest), Select: true));
                else selectNext = true;
                continue;
            }

            if (Matches(arg, "--select", "-s"))
            {
                selectNext = true;
                continue;
            }

            if (Matches(arg, "--new-tab", "-t"))
            {
                Choose(OpenIn.NewTab);
                continue;
            }

            if (Matches(arg, "--new-pane", "-p"))
            {
                Choose(OpenIn.NewPane);
                continue;
            }

            if (IsOption(arg))
            {
                // Never fall through to "treat it as a path": a mistyped flag opening the user's
                // profile folder is worse than an error.
                errors.Add($"Unrecognized option '{arg}'.");
                continue;
            }

            targets.Add(new OpenTarget(Repair(arg), selectNext));
            selectNext = false;
        }

        if (selectNext) errors.Add("'/select' expects a path after it.");

        return new CommandLineRequest(targets, mode, errors);

        void Choose(OpenIn chosen)
        {
            // Two contradictory requests is a mistake, and picking one of them silently would make
            // it look like the flag was ignored.
            if (modeChosen && mode != chosen)
            {
                errors.Add("Choose either --new-tab or --new-pane, not both.");
                return;
            }
            mode = chosen;
            modeChosen = true;
        }
    }

    private static bool Matches(string arg, string longName, string shortName) =>
        arg.Equals(longName, StringComparison.OrdinalIgnoreCase) ||
        arg.Equals(shortName, StringComparison.OrdinalIgnoreCase);

    /// <summary>
    /// A leading '-' or '/' means a switch. A path never starts with either on Windows: a drive
    /// path starts with a letter, and a UNC path with two backslashes.
    /// </summary>
    private static bool IsOption(string arg) => arg[0] is '-' or '/';

    /// <summary>
    /// Undoes Windows' backslash-before-quote mangling. <c>"C:\Dir\"</c> on a command line reaches
    /// argv as <c>C:\Dir"</c>, because the backslash escaped the closing quote — so a trailing
    /// folder separator turns into a stray quote that no path API will accept. Everyone hits this
    /// exactly once.
    /// </summary>
    private static string Repair(string path) => path.TrimEnd('"');
}
