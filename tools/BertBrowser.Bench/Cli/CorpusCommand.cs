using BertBrowser.Bench.Corpus;

namespace BertBrowser.Bench.Cli;

/// <summary>Builds or inspects the cached synthetic corpus without running anything.</summary>
internal static class CorpusCommand
{
    public static int Execute(ArgList args)
    {
        var scale = 1;
        var rebuild = false;
        var stats = false;

        while (args.TryNext(out var arg))
        {
            switch (arg)
            {
                case "--scale": scale = Math.Max(1, args.Int(arg)); break;
                case "--rebuild": rebuild = true; break;
                case "--stats": stats = true; break;
                case "--repo": args.Value(arg); break;
                default: throw ArgList.Unknown("corpus", arg);
            }
        }

        var root = BenchCorpus.DefaultRoot;
        BenchCorpus.EnsureAll(root, scale, Console.WriteLine, rebuild);

        Console.WriteLine($"database: {BenchCorpus.DbPath(root, scale)}");
        Console.WriteLine($"tree:     {BenchCorpus.TreeRoot(root, scale)}");
        if (stats) Console.WriteLine($"contents: {BenchCorpus.Describe(root, scale)}");

        return Exit.Ok;
    }
}
