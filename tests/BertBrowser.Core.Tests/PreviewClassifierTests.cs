using BertBrowser.Core.Services.Preview;
using Xunit;

namespace BertBrowser.Core.Tests;

public class PreviewClassifierTests
{
    private static PreviewTarget File(string name, long size = 1024, FileAttributes attributes = FileAttributes.Normal) =>
        new(name, size, attributes, IsDirectory: false);

    // --- what each extension is ---

    [Theory]
    [InlineData("holiday.jpg", PreviewKind.Image)]
    [InlineData("logo.PNG", PreviewKind.Image)]
    [InlineData("shot.heic", PreviewKind.Image)]
    [InlineData("raw.cr3", PreviewKind.Image)]
    [InlineData("notes.txt", PreviewKind.Text)]
    [InlineData("Program.cs", PreviewKind.Text)]
    [InlineData("tsconfig.json", PreviewKind.Text)]
    [InlineData("build.ps1", PreviewKind.Text)]
    [InlineData("smoke.bbs", PreviewKind.Text)]
    [InlineData("clip.mp4", PreviewKind.Media)]
    [InlineData("song.flac", PreviewKind.Media)]
    [InlineData("package.zip", PreviewKind.Archive)]
    [InlineData("lib.nupkg", PreviewKind.Archive)]
    [InlineData("Segoe.ttf", PreviewKind.Font)]
    [InlineData("report.pdf", PreviewKind.Document)]
    [InlineData("budget.xlsx", PreviewKind.Document)]
    public void KindFor_MapsTheExtension(string name, PreviewKind expected) =>
        Assert.Equal(expected, PreviewClassifier.KindFor(name));

    [Fact]
    public void KindFor_TreatsAnUnknownExtensionAsADocument() =>
        // Not a refusal: the shell may well have a handler, and only it can say.
        Assert.Equal(PreviewKind.Document, PreviewClassifier.KindFor("model.qqq"));

    [Theory]
    [InlineData("choco.exe.manifest")]
    [InlineData("app.appxmanifest")]
    [InlineData("Schema.xsd")]
    [InlineData("Index.cshtml")]
    [InlineData("Native.vcxproj")]
    [InlineData("catalog.rss")]
    [InlineData("build.ninja")]
    [InlineData("messages.po")]
    public void KindFor_ReadsTheObviouslyTextualOnesAsText(string name) =>
        Assert.Equal(PreviewKind.Text, PreviewClassifier.KindFor(name));

    [Fact]
    public void ADocumentCarriesATextBudget_ForTheFallbackWhenTheShellDeclines()
    {
        // The whole point of the fallback: an unrecognised file gets read rather than refused,
        // once the shell has had its turn and produced nothing.
        var plan = PreviewClassifier.Classify(File("mystery.qqq", 5000));
        Assert.Equal(PreviewKind.Document, plan.Kind);
        Assert.Equal(5000, plan.ByteBudget);
    }

    [Fact]
    public void AnEnormousImageDowngradedToTheShellCarriesNoTextBudget() =>
        // "Too big to decode" is not an invitation to read a gigantic TIFF as text.
        Assert.Equal(0, PreviewClassifier.Classify(File("scan.tiff", PreviewClassifier.MaxImageBytes + 1)).ByteBudget);

    [Fact]
    public void ADocumentCarriesItsLanguageToo() =>
        // So a file whose extension the syntax table knows still arrives coloured if the shell
        // declines and the text fallback takes over.
        Assert.Equal(SyntaxLanguage.Xml, PreviewClassifier.Classify(File("plugin.wxs")).Language);

    [Fact]
    public void KindFor_ReadsAnExtensionlessFileAsText() =>
        Assert.Equal(PreviewKind.Text, PreviewClassifier.KindFor("LICENSE"));

    [Fact]
    public void KindFor_ReadsALeadingDotFileAsText() =>
        Assert.Equal(PreviewKind.Text, PreviewClassifier.KindFor(".gitignore"));

    [Fact]
    public void KindFor_KeepsOfficeContainersAsDocuments() =>
        // A .docx really is a zip, but the shell makes a page-one thumbnail of it and a listing
        // of its guts helps nobody.
        Assert.Equal(PreviewKind.Document, PreviewClassifier.KindFor("thesis.docx"));

    [Fact]
    public void KindFor_ReadsDotTsAsTypeScriptRatherThanTransportStream() =>
        Assert.Equal(PreviewKind.Text, PreviewClassifier.KindFor("app.ts"));

    // --- refusals ---

    [Fact]
    public void NothingSelected_IsRefused()
    {
        var plan = PreviewClassifier.Classify([]);
        Assert.Equal(PreviewRefusal.NothingSelected, plan.Refusal);
        Assert.Equal(PreviewKind.None, plan.Kind);
    }

    [Fact]
    public void SeveralSelected_IsRefused() =>
        Assert.Equal(
            PreviewRefusal.MultipleSelected,
            PreviewClassifier.Classify([File("a.txt"), File("b.txt")]).Refusal);

    [Fact]
    public void AFolder_IsRefused() =>
        Assert.Equal(
            PreviewRefusal.Folder,
            PreviewClassifier.Classify(new PreviewTarget("Pictures", 0, FileAttributes.Directory, true)).Refusal);

    public static TheoryData<FileAttributes> PlaceholderAttributes() =>
        new(FileAttributes.Offline, PreviewClassifier.RecallOnOpen, PreviewClassifier.RecallOnDataAccess);

    [Theory]
    [MemberData(nameof(PlaceholderAttributes))]
    public void ACloudPlaceholder_IsRefusedRatherThanDownloaded(FileAttributes placeholder) =>
        // The whole point: reading it would make the provider fetch the content.
        Assert.Equal(
            PreviewRefusal.NotDownloaded,
            PreviewClassifier.Classify(File("holiday.jpg", 2048, FileAttributes.Normal | placeholder)).Refusal);

    [Fact]
    public void APlaceholderIsRefusedEvenForAKindTheShellWouldHandle() =>
        Assert.Equal(
            PreviewRefusal.NotDownloaded,
            PreviewClassifier.Classify(File("report.pdf", 2048, PreviewClassifier.RecallOnDataAccess)).Refusal);

    [Fact]
    public void AnEnormousArchive_IsRefused() =>
        Assert.Equal(
            PreviewRefusal.TooLarge,
            PreviewClassifier.Classify(File("everything.zip", PreviewClassifier.MaxArchiveBytes + 1)).Refusal);

    [Fact]
    public void AnEnormousFont_IsRefused() =>
        Assert.Equal(
            PreviewRefusal.TooLarge,
            PreviewClassifier.Classify(File("suspicious.ttf", PreviewClassifier.MaxFontBytes + 1)).Refusal);

    [Fact]
    public void AnArchiveJustUnderTheCap_IsNotRefused() =>
        Assert.False(PreviewClassifier.Classify(File("big.zip", PreviewClassifier.MaxArchiveBytes)).IsRefused);

    // --- budgets and downgrades ---

    [Fact]
    public void AnEnormousImage_FallsBackToTheShellRatherThanBeingRefused()
    {
        var plan = PreviewClassifier.Classify(File("scan.tiff", PreviewClassifier.MaxImageBytes + 1));
        Assert.False(plan.IsRefused);
        Assert.Equal(PreviewKind.Document, plan.Kind);
        Assert.Equal(0, plan.ByteBudget);
    }

    [Fact]
    public void AHugeLog_IsTruncatedRatherThanRefused()
    {
        var plan = PreviewClassifier.Classify(File("server.log", 40L << 30));
        Assert.Equal(PreviewKind.Text, plan.Kind);
        Assert.False(plan.IsRefused);
        Assert.Equal(PreviewClassifier.DefaultTextBudget, plan.ByteBudget);
    }

    [Fact]
    public void ASmallTextFile_BudgetsOnlyItsOwnLength() =>
        Assert.Equal(300, PreviewClassifier.Classify(File("notes.txt", 300)).ByteBudget);

    [Fact]
    public void TheTextBudgetIsTheCallersToSet() =>
        Assert.Equal(64, PreviewClassifier.Classify(File("notes.txt", 5000), textBudget: 64).ByteBudget);

    [Fact]
    public void MediaReadsNothingItself() =>
        // The shell streams it, and there is no sense in which a video is text.
        Assert.Equal(0, PreviewClassifier.Classify(File("clip.mp4", 900_000_000)).ByteBudget);

    [Fact]
    public void ATextPlanCarriesItsLanguage() =>
        Assert.Equal(SyntaxLanguage.CSharp, PreviewClassifier.Classify(File("Program.cs")).Language);

    [Fact]
    public void APlainTextPlanCarriesNoLanguage() =>
        Assert.Equal(SyntaxLanguage.None, PreviewClassifier.Classify(File("notes.txt")).Language);

    [Fact]
    public void OneSelectedItem_IsClassifiedAsItself() =>
        Assert.Equal(PreviewKind.Image, PreviewClassifier.Classify([File("a.png")]).Kind);
}
