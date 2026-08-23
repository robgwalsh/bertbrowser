using BertBrowser.Core.Services.NewItem;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The half of the ShellNew read that has judgement in it — which is why it lives in Core, in a
/// project that cannot open a registry key. The rules worth guarding are what gets dropped (a
/// registry-supplied command line, above all) and what a label falls back to when Windows will not
/// tell us one.
/// </summary>
public sealed class ShellNewImportTests
{
    private readonly HashSet<string> _files = new(StringComparer.OrdinalIgnoreCase);
    private readonly List<string> _roots = [@"C:\Templates"];

    /// <summary>A resolver that can never load anything — the interesting case.</summary>
    private static string? Unresolvable(string reference) => null;

    private IReadOnlyList<NewFileTemplate> Import(
        IEnumerable<ShellNewEntry> entries, Func<string, string?>? resolver = null) =>
        ShellNewImport.ToTemplates(
            entries,
            resolver ?? Unresolvable,
            _files.Contains,
            _roots,
            (extension, _) => $@"C:\saved\{extension.TrimStart('.')}");

    // --- what is dropped ---

    [Fact]
    public void ACommandEntryIsDropped_BecauseItWouldRunAProgram()
    {
        var entries = new[]
        {
            new ShellNewEntry(".lnk", "Shortcut", ShellNewKind.Command),
            new ShellNewEntry(".txt", "Text Document", ShellNewKind.NullFile),
        };

        var templates = Import(entries);

        Assert.Equal([".txt"], templates.Select(t => t.Extension));
    }

    [Fact]
    public void ATemplateInNoneOfTheRootsIsDropped_RatherThanOfferingATypeThatWouldAlwaysRefuse()
    {
        var entry = new ShellNewEntry(".rtf", "Rich Text", ShellNewKind.FileName, "winword.rtf");
        Assert.Empty(Import([entry]));
    }

    [Theory]
    [InlineData("")]
    [InlineData("txt")]
    [InlineData(".")]
    [InlineData(".tar.gz")]
    [InlineData(".a?b")]
    public void AnExtensionThatCouldNotEndALegalFileNameIsDropped(string extension) =>
        Assert.Empty(Import([new ShellNewEntry(extension, "Something", ShellNewKind.NullFile)]));

    [Fact]
    public void TwoEntriesForOneExtension_KeepOnlyTheFirst()
    {
        var entries = new[]
        {
            new ShellNewEntry(".txt", "Text Document", ShellNewKind.NullFile),
            new ShellNewEntry(".TXT", "Something Else", ShellNewKind.NullFile),
        };

        var templates = Import(entries);

        Assert.Equal("Text Document", Assert.Single(templates).Label);
    }

    // --- labels ---

    [Fact]
    public void AnIndirectStringIsResolvedThroughTheResolver()
    {
        var entry = new ShellNewEntry(
            ".txt", @"@%SystemRoot%\system32\notepad.exe,-469", ShellNewKind.NullFile);

        var templates = Import([entry], _ => "Text Document");

        Assert.Equal("Text Document", Assert.Single(templates).Label);
    }

    [Fact]
    public void AnIndirectStringTheResolverCannotLoad_FallsBackToTheBareExtension()
    {
        // Never the raw "@%SystemRoot%\..." text: that is the failure a user would actually see.
        var entry = new ShellNewEntry(
            ".txt", @"@%SystemRoot%\system32\notepad.exe,-469", ShellNewKind.NullFile);

        Assert.Equal("TXT File", Assert.Single(Import([entry])).Label);
    }

    [Fact]
    public void AnEntryWithNoLabelAtAll_FallsBackToTheBareExtension() =>
        Assert.Equal(
            "MD File",
            Assert.Single(Import([new ShellNewEntry(".md", null, ShellNewKind.NullFile)])).Label);

    // --- templates ---

    [Fact]
    public void ABareTemplateNameIsResolvedAgainstTheTemplateFolders()
    {
        _files.Add(@"C:\Templates\winword.rtf");
        var entry = new ShellNewEntry(".rtf", "Rich Text", ShellNewKind.FileName, "winword.rtf");

        Assert.Equal(@"C:\Templates\winword.rtf", Assert.Single(Import([entry])).TemplatePath);
    }

    [Fact]
    public void ARootedTemplateNameIsTakenAsGiven()
    {
        _files.Add(@"D:\Other\sheet.xlsx");
        var entry = new ShellNewEntry(
            ".xlsx", "Worksheet", ShellNewKind.FileName, @"D:\Other\sheet.xlsx");

        Assert.Equal(@"D:\Other\sheet.xlsx", Assert.Single(Import([entry])).TemplatePath);
    }

    [Fact]
    public void ANullFileEntryCarriesNoTemplate_BecauseItIsMeantToBeEmpty() =>
        Assert.Null(
            Assert.Single(Import([new ShellNewEntry(".txt", "Text", ShellNewKind.NullFile)]))
                .TemplatePath);

    [Fact]
    public void ADataEntryIsWrittenOutOnceAndCarriedAsAPath()
    {
        var entry = new ShellNewEntry(".xml", "XML", ShellNewKind.Data, Data: [1, 2, 3]);
        Assert.Equal(@"C:\saved\xml", Assert.Single(Import([entry])).TemplatePath);
    }

    // --- merging into what the user already has ---

    [Fact]
    public void ImportingKeepsTheTypesTheUserAlreadyHasAndTheirOrder()
    {
        List<NewFileTemplate> existing =
        [
            new() { Label = "Mine", Extension = ".md" },
            new() { Label = "Also mine", Extension = ".txt" },
        ];
        List<NewFileTemplate> imported = [new() { Label = "Theirs", Extension = ".json" }];

        var merged = ShellNewImport.Merge(existing, imported);

        Assert.Equal([".md", ".txt", ".json"], merged.Select(t => t.Extension));
        Assert.Equal("Mine", merged[0].Label);
    }

    [Fact]
    public void ImportingAddsNothingForAnExtensionAlreadyListed()
    {
        List<NewFileTemplate> existing = [new() { Label = "Mine", Extension = ".txt" }];
        List<NewFileTemplate> imported = [new() { Label = "Text Document", Extension = ".TXT" }];

        var merged = ShellNewImport.Merge(existing, imported);

        Assert.Equal("Mine", Assert.Single(merged).Label);
    }
}
