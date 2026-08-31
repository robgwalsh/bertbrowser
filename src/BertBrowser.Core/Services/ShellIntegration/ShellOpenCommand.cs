namespace BertBrowser.Core.Services.ShellIntegration;

/// <summary>A registered open command, split into the program to start and the arguments to give
/// it.</summary>
public sealed record ShellOpenCommand(string Executable, string Arguments);

/// <summary>
/// Reading the command Windows would run to open a file, so it can be run with an administrator
/// token instead.
/// </summary>
/// <remarks>
/// <para>
/// This is what makes "Run as administrator" mean something for a file that is not a program.
/// <c>.sln</c> has no <c>runas</c> verb of its own — nothing does except a handful of executable
/// types — but it does have an open command, <c>"…\VSLauncher.exe" "%1"</c>, and starting
/// <em>that</em> elevated is exactly what the user meant.
/// </para>
/// <para>
/// <b>It refuses far more than it accepts, and deliberately.</b> Everything here ends in starting a
/// program with a token, so anything it cannot read confidently is answered with null and the item
/// is greyed out. A command whose program cannot be found, or that never says where the file goes,
/// is not one to guess at.
/// </para>
/// </remarks>
public static class ShellOpenCommandParser
{
    /// <summary>The placeholders the shell substitutes the file's path for. Matched exactly, so
    /// <c>%SystemRoot%</c> is not mistaken for one — environment variables are expanded by the
    /// caller before the string arrives here.</summary>
    private static readonly string[] Placeholders = ["%1", "%L", "%l", "%V", "%v", "%D", "%d", "%*"];

    /// <param name="command">The registered command, environment variables already expanded.</param>
    /// <param name="exists">Whether a path names a file. Injected for the reason
    /// <c>UniquePath</c> and <c>ArchivePath</c> inject theirs: the decision is testable without a
    /// disk, and the awkward case below genuinely needs to ask.</param>
    /// <returns>Null when there is nothing safe to start.</returns>
    public static ShellOpenCommand? Parse(string? command, string filePath, Func<string, bool> exists)
    {
        if (string.IsNullOrWhiteSpace(command) || string.IsNullOrWhiteSpace(filePath)) return null;

        var (executable, arguments) = Split(command.Trim(), exists);
        if (executable is null) return null;
        if (!exists(executable)) return null;

        // No placeholder means the command never asked for the file. Appending it anyway would hand
        // a program an argument it does not expect, elevated — so this is refused rather than
        // guessed at. It is mostly the DDE-era entries, which could not be started this way anyway.
        var placeholder = Placeholders.FirstOrDefault(p => arguments.Contains(p, StringComparison.Ordinal));
        if (placeholder is null) return null;

        return new ShellOpenCommand(executable, arguments.Replace(placeholder, filePath, StringComparison.Ordinal));
    }

    /// <summary>
    /// The program and the rest, which is only obvious when the program is quoted.
    /// </summary>
    /// <remarks>
    /// An unquoted path containing spaces is genuinely ambiguous — <c>C:\Program Files\A B\x.exe %1</c>
    /// could be a program called <c>C:\Program</c> — and Windows resolves it by probing each
    /// candidate. The same is done here, longest first, because taking the first space would launch
    /// something other than the handler; and if nothing matches, null, because the one thing not to
    /// do with a token in hand is start a guess.
    /// </remarks>
    private static (string? Executable, string Arguments) Split(string command, Func<string, bool> exists)
    {
        if (command.StartsWith('"'))
        {
            var close = command.IndexOf('"', 1);
            if (close < 0) return (null, "");

            return (command[1..close], command[(close + 1)..].TrimStart());
        }

        // Unquoted: try the longest prefix that names a file, then shorter ones.
        for (var space = command.LastIndexOf(' '); space > 0; space = command.LastIndexOf(' ', space - 1))
        {
            var candidate = command[..space];
            if (exists(candidate)) return (candidate, command[(space + 1)..].TrimStart());
        }

        return exists(command) ? (command, "") : (null, "");
    }
}
