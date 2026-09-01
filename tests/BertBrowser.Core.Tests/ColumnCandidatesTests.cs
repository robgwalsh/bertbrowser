using BertBrowser.Core.Services.Columns;
using Xunit;

namespace BertBrowser.Core.Tests;

public class ColumnCandidatesTests
{
    private static ColumnSetting C(string id) => new(id, 100);

    /// <summary>A stand-in for what propsys.dll hands back, so this can run off Windows shells.</summary>
    private static readonly (string Canonical, string Display)[] Machine =
    [
        ("System.Photo.DateTaken", "Date taken"),
        ("System.DateAcquired", "Date acquired"),
        ("System.DateArchived", "Date archived"),
        ("System.Music.Artist", "Contributing artists"),
        ("System.ItemTypeText", "Type"),
        ("System.Size", "Size"),
        ("System.IsFolder", "Folder"),
    ];

    private static ColumnCandidates Build(
        IReadOnlyList<ColumnSetting>? layout = null,
        string search = "",
        IReadOnlyList<(string, string)>? machine = null,
        bool loaded = true) =>
        ColumnCandidates.Build(layout, machine ?? Machine, search, propertiesLoaded: loaded);

    private static string[] Ids(IEnumerable<ColumnCandidate> candidates) =>
        candidates.Select(c => c.Id).ToArray();

    private static string[] Ids(ColumnCandidateGroup group) => Ids(group.Items);

    private static ColumnCandidateGroup Common(ColumnCandidates candidates) =>
        candidates.Groups.Single(g => g.Kind == ColumnCandidateKind.Common);

    private static ColumnCandidateGroup All(ColumnCandidates candidates) =>
        candidates.Groups.Single(g => g.Kind == ColumnCandidateKind.All);

    // --- What the two groups hold ---

    [Fact]
    public void CommonLeadsWithTheBuiltInsInCatalogueOrder()
    {
        // Catalogue order, not alphabetical: it is the order a column set is usually built in, and
        // sorting would put Accessed above Modified.
        var common = Ids(Common(Build()));

        Assert.Equal(["Created", "Accessed", "Attributes", "Extension"], common[..4]);
    }

    [Fact]
    public void CommonThenCarriesTheCuratedProperties()
    {
        var common = Common(Build()).Items;

        Assert.Contains(common, c => c.Id == "System.Photo.DateTaken");
        Assert.All(common.SkipWhile(c => c.Kind == ColumnKind.BuiltIn),
            c => Assert.Equal(ColumnKind.ShellProperty, c.Kind));
    }

    [Fact]
    public void AColumnAlreadyInTheLayoutIsNotOfferedAgain()
    {
        // The popup adds; it never removes. A row that is already showing would do nothing.
        var candidates = Build([C("Name"), C("Size"), C("Created"), C("System.Photo.DateTaken")]);

        Assert.DoesNotContain("Created", Ids(Common(candidates)));
        Assert.DoesNotContain("System.Photo.DateTaken", Ids(Common(candidates)));
        Assert.DoesNotContain("System.Photo.DateTaken", Ids(All(candidates)));
    }

    [Fact]
    public void TheInjectedColumnsAreNeverOffered()
    {
        // Folder and Match follow the list's mode, not anyone's choice.
        var common = Ids(Common(Build()));

        Assert.DoesNotContain("RelativePath", common);
        Assert.DoesNotContain("Match", common);
    }

    [Fact]
    public void APropertyThatWouldShadowABuiltInIsNotOffered()
    {
        var all = Ids(All(Build()));

        Assert.DoesNotContain("System.ItemTypeText", all);
        Assert.DoesNotContain("System.Size", all);
    }

    [Fact]
    public void APropertyAlreadyOnTheCuratedListIsNotRepeatedUnderAllProperties()
    {
        var candidates = Build();

        Assert.Contains("System.Photo.DateTaken", Ids(Common(candidates)));
        Assert.DoesNotContain("System.Photo.DateTaken", Ids(All(candidates)));
    }

    [Fact]
    public void AllPropertiesIsSortedByTheLocalisedNameTheShellGaveBack() =>
        Assert.Equal(
            ["Date acquired", "Date archived", "Folder"],
            All(Build()).Items.Select(c => c.Header).ToArray());

    [Fact]
    public void APropertyWhoseNameIsNotCanonicalIsRefused()
    {
        // The same rule that keeps a hand-edited settings file from putting arbitrary text into a
        // header: no dot means it is a built-in id, and the shell has no business minting those.
        var candidates = Build(machine: [("NotCanonical", "Bare word"), ("System.Ok", "Fine")]);

        Assert.Equal(["System.Ok"], Ids(All(candidates)));
    }

    [Fact]
    public void TheDisplayNameTheShellGaveBackWinsOverTheSyntheticOne()
    {
        // ColumnCatalog.SpecForProperty shortens the canonical name as a fallback; when the machine
        // has a localised name it is the better label and the whole reason the list is enumerated.
        var taken = Common(Build()).Items.Single(c => c.Id == "System.Photo.DateTaken");

        Assert.Equal("Date taken", taken.Header);
    }

    // --- Searching ---

    [Fact]
    public void SearchMatchesTheDisplayedNameInEitherGroup()
    {
        var candidates = Build(search: "date");

        Assert.Contains("System.Photo.DateTaken", Ids(Common(candidates)));
        Assert.Equal(["System.DateAcquired", "System.DateArchived"], Ids(All(candidates)));
    }

    [Fact]
    public void SearchAlsoMatchesTheCanonicalName()
    {
        // The canonical name is what ends up in settings.json and is shown on the row, so it has to
        // be searchable too — nothing about the label "Folder" contains the word searched for here.
        Assert.Equal(["System.IsFolder"], Ids(All(Build(search: "isfolder"))));
    }

    [Fact]
    public void SearchIgnoresCaseAndSurroundingSpace() =>
        Assert.Equal(Ids(All(Build(search: "DATE"))), Ids(All(Build(search: "  date  "))));

    [Fact]
    public void SearchFindsABuiltInByItsHeader() =>
        Assert.Equal(["Attributes"], Ids(Common(Build(search: "attributes"))));

    [Fact]
    public void AGroupWithNothingLeftInItIsDropped()
    {
        // "acquired" is a word only one enumerated property has, and no built-in or curated one does.
        var candidates = Build(search: "acquired");

        Assert.Equal([ColumnCandidateKind.All], candidates.Groups.Select(g => g.Kind).ToArray());
    }

    [Fact]
    public void ASearchThatMatchesNothingReturnsNoGroupsAndSaysSo()
    {
        var candidates = Build(search: "zzzz");

        Assert.Empty(candidates.Groups);
        Assert.True(candidates.IsEmpty);
    }

    // --- While the property system is still being read ---

    [Fact]
    public void CommonIsOfferedBeforeTheMachineHasBeenEnumerated()
    {
        // A few hundred COM activations happen off the UI thread; the curated list needs no machine
        // at all, so it renders first rather than the popup opening blank.
        var candidates = Build(machine: [], loaded: false);

        Assert.Contains("Created", Ids(Common(candidates)));
        Assert.DoesNotContain(candidates.Groups, g => g.Kind == ColumnCandidateKind.All);
        Assert.True(candidates.IsLoading);
        Assert.False(candidates.IsEmpty);
    }

    [Fact]
    public void OnceLoadedAMachineWithNoPropertiesIsNotStillLoading()
    {
        var candidates = Build(machine: [], loaded: true);

        Assert.False(candidates.IsLoading);
        Assert.Contains("Created", Ids(Common(candidates)));
    }

    [Fact]
    public void ALayoutAtTheColumnLimitOffersNothing()
    {
        // MaxColumns is a hard cap; offering a row that Normalize would silently drop would read as
        // the popup being broken.
        var full = Enumerable.Range(0, ColumnLayoutRules.MaxColumns)
            .Select(i => C(i == 0 ? "Name" : $"System.Filler{i}"))
            .ToList();

        var candidates = Build(full);

        Assert.Empty(candidates.Groups);
        Assert.True(candidates.IsFull);
    }
}
