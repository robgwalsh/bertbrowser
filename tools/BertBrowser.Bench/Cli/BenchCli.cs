namespace BertBrowser.Bench.Cli;

/// <summary>Exit codes, so a script can tell a regression from a broken run from a typo.</summary>
internal static class Exit
{
    public const int Ok = 0;
    public const int Regression = 1;
    public const int Errors = 2;
    public const int Usage = 64;
}

/// <summary>A command line that does not make sense. Reported with the usage text, exit 64.</summary>
internal sealed class UsageException(string message) : Exception(message);

/// <summary>The verbs and their dispatch.</summary>
internal static class BenchCli
{
    public const string ToolName = "BertBrowser.Bench";

    private const string Usage = """
        BertBrowser.Bench — measures BertBrowser and keeps the evidence.

          run       Tier A: Core micro-benchmarks (BenchmarkDotNet)
                      --job short|default      BDN job (default: default; CI uses short)
                      --filter <glob>          BDN glob over Namespace.Class.Method, repeatable
                                               e.g. --filter *PathKey* --filter *FsIndex*Search*
                      --scale <n>              synthetic corpus: n × 100,000 rows (default 1)
                      --real                   read-only run against bench/real.local.json
                      --out <file>             default bench-results/current[.real].json
                      --machine-key <name>     file baselines under this key instead of the probed one
                      --toolchain default|inprocess
                      --ci                     = --job short, plain output
                      --allow-debug            a Debug build is otherwise refused
          ui        Tier B: whole-UI scenarios through the offscreen harness
                      --scenario <name>        a tools/ui/bench-<name>.bbs, repeatable (default: all)
                      --repeat <k>             runs per scenario, each a fresh process (default 5)
                      --harness <exe>          default tools/BertBrowser.Harness/bin/Release/.../BertBrowser.Harness.exe
                      --out <file>             default bench-results/ui.json
                      --timeout <sec> --busy-timeout <ms>   forwarded (defaults 600 / 120000)
                      --machine-key <name>
          startup   Tier C: the real BertBrowser.exe, launched against a scratch data directory
                      --launches <n>           default 10; the first is cold and discarded
                      --app <exe>              default src/BertBrowser.App/bin/Release/.../BertBrowser.exe
                      --allow-running          measure even though a real BertBrowser is open
                      --out <file>             default bench-results/startup.json
                      --machine-key <name>
          compare   --baseline <file> --current <file>
                      [--time-threshold <pct>] [--alloc-threshold <pct>] [--no-time] [--no-alloc]
                      [--strict] [--summary <md-file>] [--title <text>]
                      --ci                     gates allocations only; writes $GITHUB_STEP_SUMMARY;
                                               with no baseline, writes a candidate and passes
          baseline  --from <file> (repeatable)  [--machine-key <name>]
                    records or replaces bench/baselines/<key>[.real].json, merging tiers
          report    [--check]                   regenerates docs/performance.md from the baselines
          corpus    [--scale <n>] [--rebuild] [--stats]   builds or inspects the synthetic corpus

          --repo <dir>  on any verb: the repository root (default: found from the exe's location)

        Exit codes: 0 ok · 1 regression · 2 benchmark errors or environment · 64 usage
        """;

    public static int Run(string[] args)
    {
        if (args.Length == 0 || args[0] is "--help" or "-h" or "/?" or "help")
        {
            Console.WriteLine(Usage);
            return args.Length == 0 ? Exit.Usage : Exit.Ok;
        }

        var verb = args[0];
        var rest = new ArgList(args.Skip(1).ToArray());

        try
        {
            return verb switch
            {
                "run" => RunCommand.Execute(rest),
                "ui" => UiCommand.Execute(rest),
                "startup" => StartupCommand.Execute(rest),
                "compare" => CompareCommand.Execute(rest),
                "baseline" => BaselineCommand.Execute(rest),
                "report" => ReportCommand.Execute(rest),
                "corpus" => CorpusCommand.Execute(rest),
                _ => throw new UsageException($"Unknown command '{verb}'."),
            };
        }
        catch (UsageException e)
        {
            Console.Error.WriteLine(e.Message);
            Console.Error.WriteLine();
            Console.Error.WriteLine(Usage);
            return Exit.Usage;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or InvalidDataException
                                      or InvalidOperationException)
        {
            Console.Error.WriteLine($"error: {e.Message}");
            return Exit.Errors;
        }
    }
}

/// <summary>The arguments after the verb, consumed left to right.</summary>
internal sealed class ArgList(string[] args)
{
    private int _i;

    public bool TryNext(out string arg)
    {
        if (_i < args.Length)
        {
            arg = args[_i++];
            return true;
        }

        arg = "";
        return false;
    }

    /// <summary>The value after a flag, or a usage error naming the flag.</summary>
    public string Value(string flag) =>
        _i < args.Length ? args[_i++] : throw new UsageException($"{flag} needs a value.");

    public int Int(string flag)
    {
        var text = Value(flag);
        return int.TryParse(text, System.Globalization.NumberStyles.Integer,
            System.Globalization.CultureInfo.InvariantCulture, out var value) && value >= 0
            ? value
            : throw new UsageException($"{flag} wants a non-negative whole number, got '{text}'.");
    }

    public double Double(string flag)
    {
        var text = Value(flag);
        return double.TryParse(text, System.Globalization.NumberStyles.Float,
            System.Globalization.CultureInfo.InvariantCulture, out var value) && value >= 0
            ? value
            : throw new UsageException($"{flag} wants a non-negative number, got '{text}'.");
    }

    public static UsageException Unknown(string verb, string arg) =>
        new($"{verb}: unrecognised argument '{arg}'.");
}
