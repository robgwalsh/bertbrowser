using System.Globalization;
using BertBrowser.Core.Services.Rename;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The advanced naming rule: find/replace, a case transform, a counter and a date, placed by a
/// template. Pure — no disk, and no clock, because the date rides on the source.
/// </summary>
public sealed class RenameRuleTests
{
    private static readonly DateTime Stamped = new(2026, 3, 9, 17, 4, 22);

    private static RenameSource File_(string path, DateTime? modified = null) =>
        new(path, IsDirectory: false, modified);

    private static RenameSource Dir(string path) => new(path, IsDirectory: true);

    /// <summary>The names a rule gives, with each item's own problem discarded.</summary>
    private static string[] Names(IReadOnlyList<RenameSource> sources, RenameRule rule) =>
        RenamePattern.Apply(sources, rule).Select(n => n.Name).ToArray();

    private static string Name(RenameSource source, RenameRule rule) => Names([source], rule)[0];

    // --- the template ---

    [Fact]
    public void Name_IsTheWholeFileName_AsCommandTemplateAlreadySpellsIt() =>
        Assert.Equal("report.pdf", Name(File_(@"C:\p\report.pdf"), new RenameRule("{name}")));

    [Fact]
    public void Base_IsTheNameWithoutItsExtension() =>
        Assert.Equal("report", Name(File_(@"C:\p\report.pdf"), new RenameRule("{base}")));

    [Fact]
    public void Ext_CarriesItsOwnDot() =>
        Assert.Equal("x.pdf", Name(File_(@"C:\p\report.pdf"), new RenameRule("x{ext}")));

    [Fact]
    public void BaseDotExt_DoublesTheDot_BecauseExtAlreadyHasOne()
    {
        // Legal, and Validate accepts it — the dot is in the middle, where Clean cannot reach.
        // Pinned so the token hint keeps telling people {ext} brings its own dot.
        Assert.Equal("report..pdf", Name(File_(@"C:\p\report.pdf"), new RenameRule("{base}.{ext}")));
    }

    [Fact]
    public void BaseDotExt_OnAnExtensionlessFile_LosesTheStrayDot() =>
        Assert.Equal("LICENSE", Name(File_(@"C:\p\LICENSE"), new RenameRule("{base}.{ext}")));

    [Fact]
    public void Parent_IsTheFoldersName_NeverItsPath() =>
        Assert.Equal("Photos-a.jpg", Name(File_(@"C:\Users\Rob\Photos\a.jpg"),
            new RenameRule("{parent}-{name}")));

    [Fact]
    public void TokensAreCaseInsensitive() =>
        Assert.Equal("report.pdf", Name(File_(@"C:\p\report.pdf"), new RenameRule("{NAME}")));

    [Fact]
    public void DoubledBracesAreLiteralBraces() =>
        Assert.Equal("{report}", Name(File_(@"C:\p\report.pdf"), new RenameRule("{{{base}}}")));

    [Fact]
    public void AFolderHasNoExtensionToPlace() =>
        Assert.Equal("Work", Name(Dir(@"C:\p\My.Project"), new RenameRule("Work{ext}")));

    // --- find and replace ---

    [Fact]
    public void Replace_RewritesTheStemAndLeavesTheExtension()
    {
        var rule = new RenameRule("{name}", Find: "IMG_", Replace: "Holiday");

        Assert.Equal("Holiday0001.jpg", Name(File_(@"C:\p\IMG_0001.jpg"), rule));
    }

    [Fact]
    public void Replace_IgnoresCaseUnlessAsked()
    {
        var rule = new RenameRule("{name}", Find: "img_", Replace: "Holiday");

        Assert.Equal("Holiday0001.jpg", Name(File_(@"C:\p\IMG_0001.jpg"), rule));
    }

    [Fact]
    public void Replace_WithMatchCase_LeavesTheOtherCasingAlone()
    {
        var rule = new RenameRule("{name}", Find: "img_", Replace: "Holiday", MatchCase: true);

        Assert.Equal("IMG_0001.jpg", Name(File_(@"C:\p\IMG_0001.jpg"), rule));
    }

    [Fact]
    public void Replace_TrimsTheSpaceItLeavesBehind()
    {
        // "report v2.txt" losing "v2" leaves "report " — and the final clean cannot see it,
        // because by then the name ends in ".txt".
        var rule = new RenameRule("{name}", Find: "v2", Replace: "");

        Assert.Equal("report.txt", Name(File_(@"C:\p\report v2.txt"), rule));
    }

    [Fact]
    public void Replace_ScopedToTheExtension_LeavesTheStemAlone()
    {
        var rule = new RenameRule("{name}", Find: "jpeg", Replace: "jpg", Scope: RenameScope.Extension);

        Assert.Equal("jpeg-holiday.jpg", Name(File_(@"C:\p\jpeg-holiday.jpeg"), rule));
    }

    [Fact]
    public void Replace_ScopedToTheWholeName_ReachesTheExtensionToo()
    {
        var rule = new RenameRule("{name}", Find: "a", Replace: "b", Scope: RenameScope.WholeName);

        Assert.Equal("bbc.tbr", Name(File_(@"C:\p\aac.tar"), rule));
    }

    [Fact]
    public void Regex_SubstitutesItsOwnGroups()
    {
        // The reason no {\1} token exists: $1 already reorders captures.
        var rule = new RenameRule("{name}", Find: @"^(.*)_(.*)$", Replace: "$2_$1", UseRegex: true);

        Assert.Equal("2026_report.txt", Name(File_(@"C:\p\report_2026.txt"), rule));
    }

    [Fact]
    public void Regex_ThatCannotCompile_IsReportedRatherThanThrown()
    {
        var rule = new RenameRule("{name}", Find: "(unclosed", Replace: "x", UseRegex: true);

        Assert.NotNull(RenamePattern.ValidateRule(rule));
        // And Apply itself still answers, because the planner calls it unguarded.
        Assert.Single(RenamePattern.Apply([File_(@"C:\p\a.txt")], rule));
    }

    [Fact]
    public void Regex_ThatRunsAway_GivesUpOnThatItemInsteadOfHangingTheDialog()
    {
        // Catastrophic backtracking, three keystrokes from any Find box. The deadline is what
        // turns this into a message; without it the UI thread is gone.
        var rule = new RenameRule("{name}",
            Find: "(a+)+$", Replace: "x", UseRegex: true, Scope: RenameScope.WholeName);
        var runaway = File_(@"C:\p\" + new string('a', 40) + "b.txt");

        var result = RenamePattern.Apply([runaway], rule);

        Assert.NotNull(result[0].Problem);
    }

    // --- case ---

    [Theory]
    [InlineData(RenameCase.Lower, "holiday photo")]
    [InlineData(RenameCase.Upper, "HOLIDAY PHOTO")]
    [InlineData(RenameCase.Title, "Holiday Photo")]
    [InlineData(RenameCase.Sentence, "Holiday photo")]
    public void Case_RewritesTheStem(RenameCase kind, string expected)
    {
        var rule = new RenameRule("{base}", Case: kind);

        Assert.Equal(expected, Name(File_(@"C:\p\HOLIDAY PHOTO.jpg"), rule));
    }

    [Fact]
    public void TitleCase_WorksOnAllCaps_WhichIsWhyAnyoneReachesForIt()
    {
        // ToTitleCase leaves an already-upper-case word exactly as it found it, so this needs the
        // lower-casing pass first or the button looks dead.
        var rule = new RenameRule("{base}", Case: RenameCase.Title);

        Assert.Equal("Annual Report", Name(File_(@"C:\p\ANNUAL REPORT.docx"), rule));
    }

    [Fact]
    public void Case_IsInvariant_SoATurkishMachineDoesNotMangleTheBatch()
    {
        // Under tr-TR, "FILE".ToLower() is "fıle" with a dotless i — a different name, on every
        // item, on somebody else's computer. PathKey keeps the same discipline for the same reason.
        var original = CultureInfo.CurrentCulture;
        try
        {
            CultureInfo.CurrentCulture = new CultureInfo("tr-TR");
            var rule = new RenameRule("{base}", Case: RenameCase.Lower);

            Assert.Equal("file", Name(File_(@"C:\p\FILE.txt"), rule));
        }
        finally
        {
            CultureInfo.CurrentCulture = original;
        }
    }

    [Fact]
    public void Case_LeavesTheExtensionAloneUnlessScopedToIt()
    {
        var rule = new RenameRule("{name}", Case: RenameCase.Lower);

        Assert.Equal("report.TXT", Name(File_(@"C:\p\REPORT.TXT"), rule));
    }

    // --- the counter ---

    [Fact]
    public void Counter_NumbersInTheOrderGiven()
    {
        var rule = new RenameRule("Photo {n}{ext}");

        Assert.Equal(["Photo 1.jpg", "Photo 2.png"],
            Names([File_(@"C:\p\a.jpg"), File_(@"C:\p\b.png")], rule));
    }

    [Fact]
    public void Counter_HonoursStartAndStep()
    {
        var rule = new RenameRule("Photo {n}{ext}", CounterStart: 10, CounterStep: 5);

        Assert.Equal(["Photo 10.jpg", "Photo 15.jpg"],
            Names([File_(@"C:\p\a.jpg"), File_(@"C:\p\b.jpg")], rule));
    }

    [Fact]
    public void Counter_PadsToTheWidthWrittenInTheToken()
    {
        var rule = new RenameRule("Photo {n:000}{ext}");

        Assert.Equal(["Photo 001.jpg", "Photo 002.jpg"],
            Names([File_(@"C:\p\a.jpg"), File_(@"C:\p\b.jpg")], rule));
    }

    [Fact]
    public void Counter_CountingDown_KeepsItsSignOutsideThePadding()
    {
        var rule = new RenameRule("n{n:000}", CounterStart: 1, CounterStep: -2);

        Assert.Equal(["n001", "n-001"], Names([File_(@"C:\p\a"), File_(@"C:\p\b")], rule));
    }

    [Fact]
    public void Counter_WithAStepOfZero_IsRefusedRatherThanCollidingLater() =>
        Assert.NotNull(RenamePattern.ValidateRule(new RenameRule("{n}", CounterStep: 0)));

    [Fact]
    public void ATemplateWithoutACounter_IsNotNumbered()
    {
        // The plain box numbers a multi-item rename; a template that placed no counter must not,
        // or a find/replace across twenty files would silently number them.
        var rule = new RenameRule("{name}", Find: "IMG_", Replace: "");

        Assert.Equal(["0001.jpg", "0002.jpg"],
            Names([File_(@"C:\p\IMG_0001.jpg"), File_(@"C:\p\IMG_0002.jpg")], rule));
    }

    // --- the date ---

    [Fact]
    public void Modified_DefaultsToASortableDate() =>
        Assert.Equal("2026-03-09 report.txt",
            Name(File_(@"C:\p\report.txt", Stamped), new RenameRule("{modified} {name}")));

    [Fact]
    public void Modified_TakesAFormat() =>
        Assert.Equal("2026-03.txt",
            Name(File_(@"C:\p\report.txt", Stamped), new RenameRule("{modified:yyyy-MM}{ext}")));

    [Fact]
    public void Modified_OnAnItemWithNoDate_IsReportedRatherThanStampedYearOne()
    {
        // Search results arrive without a timestamp until they are hydrated, and a flattened
        // search result is a perfectly ordinary place to rename from.
        var result = RenamePattern.Apply(
            [File_(@"C:\p\report.txt")], new RenameRule("{modified} {name}"));

        Assert.NotNull(result[0].Problem);
    }

    [Fact]
    public void Modified_WithAFormatThatIsNotOne_IsRefused() =>
        Assert.NotNull(RenamePattern.ValidateRule(new RenameRule("{modified:%}")));

    [Fact]
    public void Modified_WithAFormatThatProducesSlashes_SaysSoInsteadOfBlamingTheCharacter()
    {
        // "d" is a valid format and gives 03/09/2026. The refusal that followed would have named
        // the slash rather than the format that put it there.
        var problem = RenamePattern.ValidateRule(new RenameRule("{modified:d}"));

        Assert.NotNull(problem);
        Assert.Contains("yyyy-MM-dd", problem);
    }

    // --- refusals ---

    [Fact]
    public void AnUnknownToken_IsRefused_AndSaysHowToTypeALiteralBrace()
    {
        var problem = RenamePattern.ValidateRule(new RenameRule("{nmae}.txt"));

        Assert.NotNull(problem);
        Assert.Contains("{{", problem);
    }

    [Theory]
    [InlineData("{name")]
    [InlineData("name}")]
    [InlineData("{name:x}")]
    [InlineData("{n:xyz}")]
    public void UnusableTemplates_AreRefusedWithAReason(string template) =>
        Assert.False(string.IsNullOrWhiteSpace(
            RenamePattern.ValidateRule(new RenameRule(template))));

    [Fact]
    public void ALiteralRule_IsNeverJudgedAsATemplate() =>
        Assert.Null(RenamePattern.ValidateRule(RenameRule.Simple("{6B99A0C1}.tmp")));

    [Fact]
    public void AGoodRule_HasNothingToSay() =>
        Assert.Null(RenamePattern.ValidateRule(
            new RenameRule("{base} {n:000}{ext}", Find: "a", Replace: "b", UseRegex: true)));
}
