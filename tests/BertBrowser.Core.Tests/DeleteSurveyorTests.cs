using BertBrowser.Core.Services.Delete;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// What the confirmation says is about to go. These numbers are the whole point of the "are you
/// sure" — "delete 1 item" and "delete 1 folder holding 4,000 files" are different questions — so
/// they are asserted against real trees rather than assumed.
/// </summary>
public sealed class DeleteSurveyorTests : IDisposable
{
    private readonly string _root;
    private readonly DeletePlanner _planner = new(new FileSystemDeleteProbe(), []);
    private readonly DeleteSurveyor _surveyor = new();

    public DeleteSurveyorTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-survey-{Guid.NewGuid():N}");
        Directory.CreateDirectory(_root);
    }

    public void Dispose()
    {
        try
        {
            Directory.Delete(_root, recursive: true);
        }
        catch (IOException)
        {
        }
    }

    private string File_(string content, params string[] parts)
    {
        var path = Path.Combine([_root, .. parts]);
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, content);
        return path;
    }

    private DeleteSurvey Survey(params string[] sources) =>
        _surveyor.Survey(_planner.Plan(
            sources.Select(s => new DeleteSource(s, Directory.Exists(s))).ToList(), permanent: false));

    [Fact]
    public void AFileIsOneFileAndItsOwnLength()
    {
        var file = File_("hello", "a.txt");

        var survey = Survey(file);

        Assert.Equal(1, survey.Files);
        Assert.Equal(0, survey.Directories);
        Assert.Equal(new FileInfo(file).Length, survey.Bytes);
        Assert.False(survey.Incomplete);
    }

    [Fact]
    public void AFolderCountsItselfAndEverythingUnderIt()
    {
        File_("one", "tree", "a.txt");
        File_("two", "tree", "deep", "b.txt");
        File_("three", "tree", "deep", "deeper", "c.txt");

        var survey = Survey(Path.Combine(_root, "tree"));

        Assert.Equal(3, survey.Files);
        Assert.Equal(3, survey.Directories); // tree, deep, deeper
        Assert.Equal(11, survey.Bytes);      // "one" + "two" + "three"
    }

    [Fact]
    public void AnEmptyFolderIsOneDirectoryAndNothingElse()
    {
        var empty = Directory.CreateDirectory(Path.Combine(_root, "empty")).FullName;

        var survey = Survey(empty);

        Assert.Equal(0, survey.Files);
        Assert.Equal(1, survey.Directories);
        Assert.Equal(0, survey.Bytes);
    }

    [Fact]
    public void SeveralItemsAddUp()
    {
        var a = File_("aaaa", "a.txt");
        File_("bb", "tree", "b.txt");

        var survey = Survey(a, Path.Combine(_root, "tree"));

        Assert.Equal(2, survey.Items.Count);
        Assert.Equal(2, survey.Files);
        Assert.Equal(1, survey.Directories);
        Assert.Equal(6, survey.Bytes);
    }

    [Fact]
    public void EachItemIsReportedAsItIsFinished()
    {
        var a = File_("a", "a.txt");
        var b = File_("b", "b.txt");

        var reports = new SynchronousProgress<DeleteMeasurement>();
        var plan = _planner.Plan(
            [new DeleteSource(a, false), new DeleteSource(b, false)], permanent: false);
        _surveyor.Survey(plan, CancellationToken.None, reports);

        Assert.Equal([a, b], reports.Reports.Select(r => r.SourcePath));
    }

    [Fact]
    public void AnItemThatHasGoneIsMarkedIncompleteRatherThanThrowing()
    {
        var file = File_("hello", "a.txt");
        var plan = _planner.Plan([new DeleteSource(file, false)], permanent: false);
        File.Delete(file);

        var survey = _surveyor.Survey(plan);

        Assert.True(survey.Incomplete);
        Assert.Equal(0, survey.Bytes);
    }

    [Fact]
    public void CancellationStopsTheWalkAndSaysSo()
    {
        File_("one", "tree", "a.txt");
        var plan = _planner.Plan(
            [new DeleteSource(Path.Combine(_root, "tree"), true)], permanent: false);

        using var cts = new CancellationTokenSource();
        cts.Cancel();
        var survey = _surveyor.Survey(plan, cts.Token);

        Assert.Empty(survey.Items);
    }
}
