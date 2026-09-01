using BertBrowser.Core.Services.Columns;
using Xunit;

namespace BertBrowser.Core.Tests;

public class ColumnLayoutRulesTests
{
    private static ColumnSetting C(string id, double width = 100) => new(id, width);

    private static string[] Ids(IEnumerable<ResolvedColumn> columns) => columns.Select(c => c.Id).ToArray();

    private static string[] Ids(IEnumerable<ColumnSetting> settings) => settings.Select(c => c.Id).ToArray();

    private static IReadOnlyList<ResolvedColumn> Listing(IReadOnlyList<ColumnSetting>? user) =>
        ColumnLayoutRules.Resolve(user, isFlattened: false, showsContentMatches: false);

    // --- What a saved list resolves to ---

    [Fact]
    public void NeverConfiguredShipsTodaysColumnsAtTodaysWidths()
    {
        var columns = Listing(null);

        Assert.Equal(["Name", "Size", "Type", "Modified"], Ids(columns));
        Assert.Equal([320d, 110, 120, 140], columns.Select(c => c.Width).ToArray());
    }

    [Fact]
    public void AnEmptyListIsHonouredRatherThanTreatedAsUnconfigured()
    {
        // Deliberately unlike NewFileTypes, where empty means "shipped defaults, all removed". Here
        // the Name rule below makes an empty layout a usable one, so empty can mean empty.
        var columns = Listing([]);

        Assert.Equal(["Name"], Ids(columns));
    }

    [Fact]
    public void NameIsPutBackWhenASavedListHasLostIt() =>
        Assert.Equal(["Name", "Size"], Ids(Listing([C("Size")])));

    [Fact]
    public void NameIsMovedToTheFrontWhenItIsNotFirst() =>
        Assert.Equal(["Name", "Size", "Modified"], Ids(Listing([C("Size"), C("Modified"), C("Name")])));

    [Fact]
    public void ADuplicateCollapsesToItsFirstOccurrenceKeepingThatWidth()
    {
        var columns = Listing([C("Name"), C("Size", 200), C("Size", 900)]);

        Assert.Equal(["Name", "Size"], Ids(columns));
        Assert.Equal(200, columns[1].Width);
    }

    [Fact]
    public void TheColumnCountIsCapped()
    {
        // Every curated property, several times over — a hand-edited file should not be able to make
        // a row realize hundreds of cells.
        var many = Enumerable.Repeat(ColumnCatalog.Curated, 4).SelectMany(c => c)
            .Select((s, i) => new ColumnSetting($"{s.Id}{i}", 100)).ToList();
        many.InsertRange(0, ColumnCatalog.Curated.Select(s => C(s.Id)));

        Assert.True(Listing(many).Count <= ColumnLayoutRules.MaxColumns);
    }

    // --- Which ids survive ---

    [Fact]
    public void AnUnknownBuiltInIdIsDropped() =>
        // A bare word this build does not know came from a newer one and names a column it cannot
        // render. Keeping it would mean a blank column nobody can explain or remove.
        Assert.Equal(["Name", "Size"], Ids(Listing([C("Name"), C("Colour"), C("Size")])));

    [Fact]
    public void AnUnrecognisedCanonicalNameIsKeptAndRendersBlank()
    {
        // The other half of the same rule: this machine may simply have no handler for it, and
        // unknown is blank, never wrong.
        var columns = Listing([C("Name"), C("System.Contact.NickName")]);

        Assert.Equal(["Name", "System.Contact.NickName"], Ids(columns));
        Assert.Equal(ColumnKind.ShellProperty, columns[1].Spec.Kind);
    }

    [Theory]
    [InlineData("RelativePath")]
    [InlineData("Match")]
    public void AnInjectedColumnInASavedListIsIgnored(string id) =>
        // Folder and Match follow the list's mode. One arriving from a hand-edited file would
        // otherwise show up twice, or show up in a listing where it means nothing.
        Assert.Equal(["Name", "Size"], Ids(Listing([C("Name"), C(id), C("Size")])));

    // --- Widths ---

    [Theory]
    [InlineData(double.NaN)]        // what a gripper double-click leaves behind
    [InlineData(double.PositiveInfinity)]
    [InlineData(0)]
    [InlineData(-40)]
    public void AnUnusableWidthBecomesTheColumnDefault(double width) =>
        Assert.Equal(110, Listing([C("Name"), C("Size", width)])[1].Width);

    [Fact]
    public void AWidthSavedOnAMuchWiderMonitorIsClamped() =>
        Assert.Equal(ColumnLayoutRules.MaxWidth, Listing([C("Name"), C("Size", 99_999)])[1].Width);

    [Fact]
    public void AWidthTooNarrowToGrabIsClamped() =>
        Assert.Equal(ColumnLayoutRules.MinWidth, Listing([C("Name"), C("Size", 1)])[1].Width);

    // --- The two injected columns ---

    [Fact]
    public void FolderAppearsOnlyInAFlattenedList()
    {
        Assert.DoesNotContain("RelativePath", Ids(Listing(null)));
        Assert.Contains("RelativePath",
            Ids(ColumnLayoutRules.Resolve(null, isFlattened: true, showsContentMatches: false)));
    }

    [Fact]
    public void MatchIsKeyedOnContentMatchesAndNotOnFlattening()
    {
        // Every search flattens, so keying Match on that would make an empty column appear the
        // moment anyone typed into the box — which reads as a rendering fault.
        var searched = ColumnLayoutRules.Resolve(null, isFlattened: true, showsContentMatches: false);
        Assert.DoesNotContain("Match", Ids(searched));

        var grepped = ColumnLayoutRules.Resolve(null, isFlattened: true, showsContentMatches: true);
        Assert.Contains("Match", Ids(grepped));
    }

    [Fact]
    public void FolderAndMatchSitBetweenTheNameAndEverythingElse() =>
        Assert.Equal(
            ["Name", "RelativePath", "Match", "Size", "Type", "Modified"],
            Ids(ColumnLayoutRules.Resolve(null, isFlattened: true, showsContentMatches: true)));

    [Fact]
    public void TheInjectedColumnsAreMarkedAsSuch()
    {
        var columns = ColumnLayoutRules.Resolve(null, isFlattened: true, showsContentMatches: true);

        Assert.All(columns.Where(c => ColumnCatalog.IsInjected(c.Id)), c => Assert.True(c.Injected));
        Assert.All(columns.Where(c => !ColumnCatalog.IsInjected(c.Id)), c => Assert.False(c.Injected));
    }

    [Fact]
    public void MatchIsNotSortable()
    {
        // Sorting a result set by the line number a needle was found on means nothing across
        // different files. This used to be said by giving that column no Tag to parse.
        var match = ColumnLayoutRules
            .Resolve(null, isFlattened: true, showsContentMatches: true)
            .Single(c => c.Id == "Match");

        Assert.False(match.Spec.Sortable);
    }

    // --- Editing ---

    [Fact]
    public void TogglingAColumnOnAddsItAtTheEnd() =>
        Assert.Equal(
            ["Name", "Size", "Type", "Modified", "System.Photo.DateTaken"],
            Ids(ColumnLayoutRules.Toggle(null, "System.Photo.DateTaken", on: true)));

    [Fact]
    public void TogglingAColumnOnTwiceAddsItOnce() =>
        Assert.Equal(
            ColumnLayoutRules.Toggle(null, "Created", on: true).Count,
            ColumnLayoutRules.Toggle(ColumnLayoutRules.Toggle(null, "Created", on: true), "Created", on: true).Count);

    [Fact]
    public void TogglingAColumnOffRemovesIt() =>
        Assert.Equal(["Name", "Size", "Modified"], Ids(ColumnLayoutRules.Toggle(null, "Type", on: false)));

    [Fact]
    public void NameCannotBeSwitchedOff() =>
        Assert.Contains("Name", Ids(ColumnLayoutRules.Toggle(null, "Name", on: false)));

    [Fact]
    public void MovingAColumnReordersIt() =>
        Assert.Equal(["Name", "Modified", "Size", "Type"], Ids(ColumnLayoutRules.Move(null, "Modified", 1)));

    [Fact]
    public void NothingCanBeMovedInFrontOfName() =>
        Assert.Equal("Name", ColumnLayoutRules.Move(null, "Modified", 0)[0].Id);

    [Fact]
    public void NameCannotBeMovedAwayFromTheFront() =>
        // AllowsColumnReorder has always let the header be dragged; persisting the layout is what
        // would have made that stick.
        Assert.Equal("Name", ColumnLayoutRules.Move(null, "Name", 3)[0].Id);

    [Fact]
    public void AnOutOfRangeMoveClampsRatherThanThrowing() =>
        Assert.Equal(["Name", "Type", "Modified", "Size"], Ids(ColumnLayoutRules.Move(null, "Size", 99)));

    [Fact]
    public void MovingAColumnThatIsNotThereChangesNothing() =>
        Assert.Equal(["Name", "Size", "Type", "Modified"], Ids(ColumnLayoutRules.Move(null, "Created", 1)));

    [Fact]
    public void SettingAWidthSanitizesIt() =>
        Assert.Equal(110, ColumnLayoutRules.SetWidth(null, "Size", double.NaN).Single(c => c.Id == "Size").Width);

    // --- The "More columns…" picker ---

    [Fact]
    public void PickingAPropertyAddsItAtTheEnd() =>
        Assert.Equal(
            ["Name", "Size", "Type", "Modified", "System.Photo.DateTaken"],
            Ids(ColumnLayoutRules.ApplyPicked(null, ["System.Photo.DateTaken"])));

    [Fact]
    public void UntickingAPropertyRemovesIt()
    {
        var withOne = ColumnLayoutRules.ApplyPicked(null, ["System.Photo.DateTaken"]);

        Assert.Equal(["Name", "Size", "Type", "Modified"], Ids(ColumnLayoutRules.ApplyPicked(withOne, [])));
    }

    [Fact]
    public void OpeningThePickerAndChangingNothingChangesNothing()
    {
        // What the rule is really for: the answer contains everything already ticked, so a naive
        // apply would remove and re-add each one and quietly shuffle them all to the end.
        IReadOnlyList<ColumnSetting> before =
            [C("Name", 400), C("System.Photo.DateTaken", 150), C("Size", 90)];

        var after = ColumnLayoutRules.ApplyPicked(before, ["System.Photo.DateTaken"]);

        Assert.Equal(Ids(before), Ids(after));
        Assert.Equal(150, after.Single(c => c.Id == "System.Photo.DateTaken").Width);
    }

    [Fact]
    public void ThePickerLeavesTheBuiltInColumnsAlone() =>
        // It lists the property system, so a built-in missing from its answer means "not offered
        // here", never "remove it".
        Assert.Equal(["Name", "Size", "Type", "Modified"], Ids(ColumnLayoutRules.ApplyPicked(null, [])));

    // --- Capturing a drag ---

    [Fact]
    public void CaptureOrderTakesTheLiveOrderAndDropsTheInjectedColumns()
    {
        string[] live = ["Name", "RelativePath", "Match", "Modified", "Size", "Type"];

        Assert.Equal(["Name", "Modified", "Size", "Type"], Ids(ColumnLayoutRules.CaptureOrder(live, null)));
    }

    [Fact]
    public void CaptureOrderKeepsTheWidthsTheColumnsAlreadyHad()
    {
        IReadOnlyList<ColumnSetting> saved = [C("Name", 400), C("Size", 200)];
        string[] live = ["Size", "Name"];

        var captured = ColumnLayoutRules.CaptureOrder(live, saved);

        Assert.Equal(400, captured.Single(c => c.Id == "Name").Width);
        Assert.Equal(200, captured.Single(c => c.Id == "Size").Width);
    }

    [Fact]
    public void CaptureOrderSnapsADraggedNameColumnBackToTheFront() =>
        Assert.Equal(["Name", "Size", "Type"], Ids(ColumnLayoutRules.CaptureOrder(["Size", "Type", "Name"], null)));

    // --- Stepping a width with the wheel ---

    [Fact]
    public void AWheelNotchMovesTheWidthByTen()
    {
        Assert.Equal(150, ColumnLayoutRules.StepWidth(140, 1, fine: false));
        Assert.Equal(130, ColumnLayoutRules.StepWidth(140, -1, fine: false));
    }

    [Fact]
    public void EachNotchOfOneEventCounts() =>
        Assert.Equal(170, ColumnLayoutRules.StepWidth(140, 3, fine: false));

    [Fact]
    public void ACoarseNotchSnapsAnOffGridWidthOntoTheGrid()
    {
        // 137 up is 140, not 147. The point of a coarse step is that spinning the wheel walks a
        // tidy sequence instead of carrying an arbitrary dragged width along with it forever.
        Assert.Equal(140, ColumnLayoutRules.StepWidth(137, 1, fine: false));
        Assert.Equal(130, ColumnLayoutRules.StepWidth(137, -1, fine: false));
    }

    [Fact]
    public void AFineNotchMovesByOneAndDoesNotSnap()
    {
        Assert.Equal(138, ColumnLayoutRules.StepWidth(137, 1, fine: true));
        Assert.Equal(136, ColumnLayoutRules.StepWidth(137, -1, fine: true));
    }

    [Fact]
    public void SteppingStopsAtTheBoundsADraggedGripperStopsAt()
    {
        Assert.Equal(ColumnLayoutRules.MinWidth, ColumnLayoutRules.StepWidth(ColumnLayoutRules.MinWidth, -1, fine: false));
        Assert.Equal(ColumnLayoutRules.MaxWidth, ColumnLayoutRules.StepWidth(ColumnLayoutRules.MaxWidth, 1, fine: false));
    }

    [Fact]
    public void SteppingAnUnusableWidthRepairsItRatherThanPropagatingIt()
    {
        // NaN is what double-clicking a gripper leaves behind, and it is a real saved value. The
        // wheel must not turn it into NaN + 10.
        Assert.Equal(110, ColumnLayoutRules.StepWidth(double.NaN, -1, fine: false));
        Assert.Equal(130, ColumnLayoutRules.StepWidth(double.PositiveInfinity, 1, fine: false));
    }

    [Fact]
    public void NoNotchesLeavesTheWidthAloneButStillRepairsIt()
    {
        Assert.Equal(137, ColumnLayoutRules.StepWidth(137, 0, fine: false));
        Assert.Equal(120, ColumnLayoutRules.StepWidth(double.NaN, 0, fine: false));
    }
}
