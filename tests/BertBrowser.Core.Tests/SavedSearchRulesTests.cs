using BertBrowser.Core.Models;
using BertBrowser.Core.Services.SavedSearches;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class SavedSearchRulesTests
{
    private static readonly Func<string, bool> NoNameTaken = _ => false;
    private static readonly Func<string, bool> NoArchives = _ => false;
    private static readonly Func<string, bool> ZipIsArchive = p => p.EndsWith(".zip", StringComparison.OrdinalIgnoreCase);

    private static string? Validate(
        string name = "Docs",
        string query = "ext:docx",
        SavedSearchScope scope = SavedSearchScope.CurrentFolder,
        string? scopePath = null,
        Func<string, bool>? nameTaken = null,
        Func<string, bool>? isArchiveFile = null) =>
        SavedSearchRules.Validate(name, query, scope, scopePath, nameTaken ?? NoNameTaken, isArchiveFile ?? NoArchives);

    // --- Validate ---

    [Fact]
    public void AValidSearchHasNoProblem() => Assert.Null(Validate());

    [Theory]
    [InlineData("")]
    [InlineData("   ")]
    public void ABlankNameIsRefused(string name) =>
        Assert.Contains("name", Validate(name: name)!, StringComparison.OrdinalIgnoreCase);

    [Fact]
    public void ATakenNameIsRefused()
    {
        var problem = Validate(name: "Docs", nameTaken: n => n == "Docs");
        Assert.NotNull(problem);
        Assert.Contains("Docs", problem);
    }

    [Fact]
    public void TheNameIsTrimmedBeforeTheTakenCheck()
    {
        var asked = new List<string>();
        Validate(name: "  Docs  ", nameTaken: n => { asked.Add(n); return false; });
        Assert.Equal(["Docs"], asked);
    }

    [Theory]
    [InlineData("")]
    [InlineData("   ")]
    public void ABlankQueryIsRefusedAsBlankNotAsTooBroad(string query)
    {
        var problem = Validate(query: query)!;
        Assert.Contains("Type a search", problem);
    }

    [Fact]
    public void AGrammarProblemIsPassedThroughVerbatim()
    {
        var expected = BertBrowser.Core.Services.Search.SearchGrammar.Parse("dc:2026").Problem;
        Assert.NotNull(expected);
        Assert.Equal(expected, Validate(query: "dc:2026"));
    }

    [Theory]
    [InlineData("a")]
    [InlineData("is:dir")]
    public void ATooBroadQueryIsRefused(string query)
    {
        // Parse returns neither a query nor a problem for these; the box would show the folder
        // listing, which as a saved search would look like a bug.
        var problem = Validate(query: query);
        Assert.NotNull(problem);
        Assert.Contains("two characters", problem);
    }

    [Fact]
    public void FolderScopeWithoutAPathIsRefused() =>
        Assert.NotNull(Validate(scope: SavedSearchScope.Folder, scopePath: null));

    [Fact]
    public void FolderScopeWithABlankPathIsRefused() =>
        Assert.NotNull(Validate(scope: SavedSearchScope.Folder, scopePath: "  "));

    [Fact]
    public void AFolderInsideAnArchiveCannotBePinned()
    {
        var problem = Validate(scope: SavedSearchScope.Folder, scopePath: @"C:\x\a.zip\src", isArchiveFile: ZipIsArchive);
        Assert.NotNull(problem);
        Assert.Contains("archive", problem, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void ARealFolderWhoseNameLooksLikeAnArchiveCanBePinned()
    {
        // The probe says C:\x.zip is not a file, so it is a folder that happens to be called that.
        Assert.Null(Validate(scope: SavedSearchScope.Folder, scopePath: @"C:\x.zip\real", isArchiveFile: NoArchives));
    }

    [Theory]
    [InlineData(SavedSearchScope.CurrentFolder)]
    [InlineData(SavedSearchScope.ThisPc)]
    public void OnlyFolderScopeCarriesAPath(SavedSearchScope scope) =>
        Assert.NotNull(Validate(scope: scope, scopePath: @"C:\x"));

    [Fact]
    public void TheFirstProblemWinsInOrder()
    {
        // Blank name outranks a bad query; a bad query outranks a missing folder.
        Assert.Contains("name", Validate(name: "", query: "dc:2026", scope: SavedSearchScope.Folder)!, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("folder", Validate(query: "dc:2026", scope: SavedSearchScope.Folder)!, StringComparison.OrdinalIgnoreCase);
    }

    // --- DefaultName ---

    [Fact]
    public void DefaultNameIsTheTrimmedQuery() =>
        Assert.Equal("ext:jpg size:>1mb", SavedSearchRules.DefaultName("  ext:jpg   size:>1mb "));

    [Fact]
    public void DefaultNameIsCapped()
    {
        var name = SavedSearchRules.DefaultName(new string('x', 200));
        Assert.True(name.Length <= 40, $"{name.Length} chars");
    }

    [Fact]
    public void DefaultNameFallsBackWhenTheQueryIsBlank() =>
        Assert.Equal("Saved search", SavedSearchRules.DefaultName("   "));

    // --- Plan ---

    [Fact]
    public void ATemplateRunsInTheCurrentFolder()
    {
        var run = SavedSearchRules.Plan(new SavedSearch("T", "ext:txt", SavedSearchScope.CurrentFolder, null), @"C:\here");
        Assert.Equal(new SavedSearchRun(@"C:\here", "ext:txt", Global: false), run);
    }

    [Fact]
    public void APinnedSearchRunsInItsFolderWhereverYouAre()
    {
        var run = SavedSearchRules.Plan(new SavedSearch("P", "ext:txt", SavedSearchScope.Folder, @"C:\pinned"), @"C:\here");
        Assert.Equal(new SavedSearchRun(@"C:\pinned", "ext:txt", Global: false), run);
    }

    [Fact]
    public void AThisPcSearchIsGlobalButStillNeedsTheTabToHaveAFolder()
    {
        var run = SavedSearchRules.Plan(new SavedSearch("G", "ext:txt", SavedSearchScope.ThisPc, null), @"C:\here");
        Assert.Equal(new SavedSearchRun(@"C:\here", "ext:txt", Global: true), run);
    }

    [Theory]
    [InlineData(SavedSearchScope.CurrentFolder)]
    [InlineData(SavedSearchScope.ThisPc)]
    public void WithNoCurrentFolderThereIsNowhereToRun(SavedSearchScope scope) =>
        Assert.Null(SavedSearchRules.Plan(new SavedSearch("X", "ext:txt", scope, null), ""));

    [Fact]
    public void APinnedSearchStillPlansWithNoCurrentFolder()
    {
        var run = SavedSearchRules.Plan(new SavedSearch("P", "ext:txt", SavedSearchScope.Folder, @"C:\pinned"), "");
        Assert.Equal(@"C:\pinned", run?.NavigateTo);
    }
}
