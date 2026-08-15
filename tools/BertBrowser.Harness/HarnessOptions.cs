namespace BertBrowser.Harness;

/// <summary>How a run was asked for.</summary>
internal sealed class HarnessOptions
{
    /// <summary>Commands to run, in order, already split into lines.</summary>
    public required IReadOnlyList<string> Commands { get; init; }

    /// <summary>Where captures land, and the root a bare file name resolves against.</summary>
    public required string OutputDir { get; init; }

    /// <summary>
    /// The throwaway tree the run browses and is allowed to modify.
    /// </summary>
    /// <remarks>
    /// This app moves, renames and deletes the user's files for a living, so a scripted run needs
    /// a fence around it. Every destructive command checks its targets against this root; see
    /// <see cref="AllowOutside"/> for the deliberate way through.
    /// </remarks>
    public required string SandboxDir { get; init; }

    /// <summary>The app's data root — database, settings and user themes — for this run alone.</summary>
    public required string StateDir { get; init; }

    /// <summary>False when the state directory is scratch and should be deleted afterwards.</summary>
    public bool KeepState { get; init; }

    /// <summary>
    /// Lets rename, move, copy and delete act on paths outside the sandbox.
    /// </summary>
    /// <remarks>
    /// Off by default and worth keeping that way. The harness drives the real transfer, rename and
    /// delete executors against the real filesystem — a mistyped path in a script is a mistyped
    /// path in a `del`.
    /// </remarks>
    public bool AllowOutside { get; init; }

    /// <summary>Seconds before the watchdog gives up on the whole run.</summary>
    public int TimeoutSeconds { get; init; } = 120;

    /// <summary>How long one listing, search or transfer may take before it is called a hang.</summary>
    public int BusyTimeoutMs { get; init; } = 30_000;

    /// <summary>Carry on after a failed command rather than stopping at the first one.</summary>
    public bool KeepGoing { get; init; }

    /// <summary>The window size every capture is taken at, so a picture does not depend on
    /// whatever bounds the last real session happened to save.</summary>
    public int WindowWidth { get; init; } = 1400;

    public int WindowHeight { get; init; } = 900;

    /// <summary>Theme to start in, or null for the app's default.</summary>
    public string? ThemeId { get; init; }

    /// <summary>Where the first tab opens. Defaults to the run's own sandbox.</summary>
    public string? StartPath { get; init; }

    /// <summary>
    /// Builds the global MFT index, as a real launch does.
    /// </summary>
    /// <remarks>
    /// Off by default, and not merely for speed. The indexer reads every NTFS volume's master file
    /// table — minutes of disk on a machine someone is using — and it needs administrator rights
    /// the harness deliberately does not ask for. Whole-PC search is the one feature a run without
    /// it cannot exercise; folder-local search does not touch it.
    /// </remarks>
    public bool Index { get; init; }

    public bool Verbose { get; init; }

    /// <summary>Exit codes, so a caller can tell a failed assertion from a broken environment.</summary>
    public static class Exit
    {
        public const int Ok = 0;
        public const int Failed = 1;
        public const int Environment = 2;
        public const int Timeout = 3;
        public const int Usage = 64;
    }

    private const string Usage = """
        BertBrowser UI harness — drives the real window offscreen, where it cannot take focus
        from you or appear over what you are doing.

          --script <path>     run the commands in a file
          -c "<commands>"     run commands given inline, separated by ';'
                              (with neither, commands are read from stdin)

          --out <dir>         where captures go        (default: %TEMP%\bertbrowser-harness\<time>)
          --sandbox <dir>     the tree a run may modify (default: <out>\sandbox)
          --state-dir <dir>   the app's data root       (default: a scratch dir, deleted after)
          --keep-state        keep the state directory, to test restore across two runs
          --allow-outside     let destructive commands act outside the sandbox
          --size <WxH>        window size for captures  (default: 1400x900)
          --theme <id>        start in a theme          (e.g. light-plus, nord)
          --start <path>      where the first tab opens (default: the sandbox)
          --index             build the MFT index, as a real launch does (needs elevation)
          --timeout <sec>     watchdog, in seconds      (default: 120)
          --busy-timeout <ms> longest allowed listing   (default: 30000)
          --keep-going        do not stop at the first failure
          --verbose
          --help

        Every command echoes 'OK <command>'. Captures print 'SHOT <path>' on their own line.
        """;

    /// <summary>Reads the command line, or explains itself and returns null.</summary>
    public static HarnessOptions? Parse(string[] args, TextWriter output, out int exitCode)
    {
        exitCode = Exit.Ok;

        string? scriptPath = null;
        string? inline = null;
        string? outputDir = null;
        string? sandboxDir = null;
        string? stateDir = null;
        string? themeId = null;
        string? startPath = null;
        var keepState = false;
        var allowOutside = false;
        var timeout = 120;
        var busyTimeout = 30_000;
        var keepGoing = false;
        var index = false;
        var verbose = false;
        var width = 1400;
        var height = 900;

        try
        {
            for (var i = 0; i < args.Length; i++)
            {
                string Next(string flag) =>
                    i + 1 < args.Length
                        ? args[++i]
                        : throw new FormatException($"{flag} needs a value.");

                switch (args[i])
                {
                    case "--script": scriptPath = Next("--script"); break;
                    case "-c" or "--command": inline = Next("-c"); break;
                    case "--out": outputDir = Next("--out"); break;
                    case "--sandbox": sandboxDir = Next("--sandbox"); break;
                    case "--state-dir": stateDir = Next("--state-dir"); break;
                    case "--keep-state": keepState = true; break;
                    case "--allow-outside": allowOutside = true; break;
                    case "--size": (width, height) = ParseSize(Next("--size")); break;
                    case "--theme": themeId = Next("--theme"); break;
                    case "--start": startPath = Path.GetFullPath(Next("--start")); break;
                    case "--index": index = true; break;
                    case "--timeout": timeout = int.Parse(Next("--timeout")); break;
                    case "--busy-timeout": busyTimeout = int.Parse(Next("--busy-timeout")); break;
                    case "--keep-going": keepGoing = true; break;
                    case "--verbose": verbose = true; break;

                    case "--help" or "-h" or "/?":
                        output.WriteLine(Usage);
                        return null;

                    default:
                        output.WriteLine($"Unrecognised argument '{args[i]}'.");
                        output.WriteLine();
                        output.WriteLine(Usage);
                        exitCode = Exit.Usage;
                        return null;
                }
            }
        }
        catch (Exception e) when (e is FormatException or OverflowException)
        {
            output.WriteLine(e.Message);
            exitCode = Exit.Usage;
            return null;
        }

        IReadOnlyList<string> commands;

        if (scriptPath is not null)
        {
            if (!File.Exists(scriptPath))
            {
                output.WriteLine($"No script at '{scriptPath}'.");
                exitCode = Exit.Environment;
                return null;
            }

            commands = File.ReadAllLines(scriptPath);
        }
        else if (inline is not null)
        {
            commands = inline.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        }
        else
        {
            commands = (Console.In.ReadToEnd() ?? "").Split('\n');
        }

        var stamp = DateTime.Now.ToString("yyyyMMdd-HHmmss");
        var root = Path.GetFullPath(
            outputDir ?? Path.Combine(Path.GetTempPath(), "bertbrowser-harness", stamp));

        return new HarnessOptions
        {
            Commands = commands,
            OutputDir = root,
            SandboxDir = Path.GetFullPath(sandboxDir ?? Path.Combine(root, "sandbox")),
            StateDir = Path.GetFullPath(stateDir ?? Path.Combine(root, "state")),
            KeepState = keepState || stateDir is not null,
            AllowOutside = allowOutside,
            TimeoutSeconds = timeout,
            BusyTimeoutMs = busyTimeout,
            KeepGoing = keepGoing,
            WindowWidth = width,
            WindowHeight = height,
            ThemeId = themeId,
            StartPath = startPath,
            Index = index,
            Verbose = verbose,
        };
    }

    private static (int Width, int Height) ParseSize(string text)
    {
        var parts = text.Split('x', 'X');
        if (parts.Length != 2 || !int.TryParse(parts[0], out var w) || !int.TryParse(parts[1], out var h))
            throw new FormatException($"--size wants WxH, got '{text}'.");

        return (w, h);
    }
}
