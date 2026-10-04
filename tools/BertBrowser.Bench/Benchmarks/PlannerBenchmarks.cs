using BenchmarkDotNet.Attributes;
using BertBrowser.Bench.Fakes;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Rename;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.Bench.Benchmarks;

/// <summary>
/// Planning a move of a thousand items, and expanding a folder-onto-folder merge of four thousand
/// descendants — the two decisions a drop makes before anything is written, both against an
/// in-memory disk so only the deciding is timed.
/// </summary>
public class TransferBenchmarks
{
    private const string Source = @"Q:\Bench\Src";
    private const string Destination = @"Q:\Bench\Dst";

    private TransferPlanner _planner = null!;
    private TransferMergeExpander _expander = null!;
    private string[] _sources = [];
    private TransferPlan _mergePlan = null!;

    [GlobalSetup]
    public void Setup()
    {
        var disk = new FakeDisk().AddDirectory(Source).AddDirectory(Destination);

        // 600 files and 400 folders, 20 of them inside another selected folder: the containment
        // check the planner makes for every pair.
        var sources = new List<string>();
        for (var i = 0; i < 600; i++)
        {
            var path = $@"{Source}\file{i:000}.txt";
            disk.AddFile(path, 4096);
            sources.Add(path);
        }
        for (var i = 0; i < 400; i++)
        {
            var path = $@"{Source}\dir{i:000}";
            disk.AddDirectory(path);
            sources.Add(path);
        }
        for (var i = 0; i < 20; i++)
        {
            var path = $@"{Source}\dir005\inner{i:00}";
            disk.AddDirectory(path);
            sources.Add(path);
        }
        _sources = sources.ToArray();

        // The merge: the same folder on both sides, 4,000 files in 40 subfolders, every one a clash.
        foreach (var side in new[] { Source, Destination })
        {
            disk.AddDirectory($@"{side}\Big");
            for (var d = 0; d < 40; d++)
            {
                disk.AddDirectory($@"{side}\Big\sub{d:00}");
                for (var f = 0; f < 100; f++)
                    disk.AddFile($@"{side}\Big\sub{d:00}\item{f:000}.dat", 2048 + d);
            }
        }

        _planner = new TransferPlanner(disk);
        _expander = new TransferMergeExpander(disk);
        _mergePlan = _planner.Plan([$@"{Source}\Big"], Destination, TransferVerb.Copy);
    }

    [Benchmark]
    public TransferPlan Plan1000() => _planner.Plan(_sources, Destination, TransferVerb.Move);

    [Benchmark]
    public TransferPlan ExpandMerge4000() => _expander.Expand(_mergePlan);
}

/// <summary>A thousand files renamed at once: the literal pattern that makes every name collide, and
/// a rule with a counter that makes none of them.</summary>
public class RenameBenchmarks
{
    private RenamePlanner _planner = null!;
    private RenameSource[] _sources = [];
    private readonly RenameRule _numbered = new("{base}-{n:0000}{ext}");

    [GlobalSetup]
    public void Setup()
    {
        var disk = new FakeDisk().AddDirectory(@"Q:\Bench\Src");
        _sources = Enumerable.Range(0, 1000).Select(i =>
        {
            var path = $@"Q:\Bench\Src\f{i:0000}.txt";
            disk.AddFile(path);
            return new RenameSource(path, false);
        }).ToArray();
        _planner = new RenamePlanner(disk);
    }

    /// <summary>Everything to one name: the vacancy loop's worst case.</summary>
    [Benchmark]
    public RenamePlan PlanLiteral1000() => _planner.Plan(_sources, "renamed.txt");

    [Benchmark]
    public RenamePlan PlanNumbered1000() => _planner.Plan(_sources, _numbered);
}

/// <summary>A thousand items planned for the Recycle Bin and for permanent deletion.</summary>
public class DeleteBenchmarks
{
    [Params(DeleteMode.Recycle, DeleteMode.Permanent)]
    public DeleteMode Mode { get; set; }

    private DeletePlanner _planner = null!;
    private DeleteSource[] _sources = [];

    [GlobalSetup]
    public void Setup()
    {
        var disk = new FakeDisk().AddDirectory(@"Q:\Bench\Src");
        var sources = new List<DeleteSource>();
        for (var i = 0; i < 900; i++)
        {
            var path = $@"Q:\Bench\Src\f{i:0000}.txt";
            disk.AddFile(path);
            sources.Add(new DeleteSource(path, false));
        }
        for (var i = 0; i < 50; i++)
        {
            var path = $@"Q:\Bench\Src\d{i:00}";
            disk.AddDirectory(path);
            sources.Add(new DeleteSource(path, true));
            var nested = $@"{path}\inner";
            disk.AddDirectory(nested);
            sources.Add(new DeleteSource(nested, true));
        }
        _sources = sources.ToArray();
        _planner = new DeletePlanner(disk, protectedPaths: null, recycleProbe: disk);
    }

    [Benchmark]
    public DeletePlan Plan1000() => _planner.Plan(_sources, Mode);
}
