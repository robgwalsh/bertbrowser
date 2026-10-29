using BertBrowser.Core.Services.Compare;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class FileComparePlanTests
{
    private static FileCompareSide Side(string name, bool looksLikeText = false) => new(name, looksLikeText);

    [Fact]
    public void TwoTextExtensionsAreText() =>
        Assert.Equal(FileCompareMode.Text, FileComparePlan.For(Side("a.cs"), Side("b.cs")));

    [Fact]
    public void TwoBinariesAreBinary() =>
        Assert.Equal(FileCompareMode.Binary, FileComparePlan.For(Side("a.exe"), Side("b.exe")));

    /// <summary>
    /// Both sides, not either. Diffing a .txt against a .exe as text would render one side as
    /// mojibake and invite a conclusion about it.
    /// </summary>
    [Fact]
    public void AMixedPairIsBinary()
    {
        Assert.Equal(FileCompareMode.Binary, FileComparePlan.For(Side("notes.txt"), Side("setup.exe")));
        Assert.Equal(FileCompareMode.Binary, FileComparePlan.For(Side("setup.exe"), Side("notes.txt")));
    }

    /// <summary>The second rung: an extensionless README or a Dockerfile has no extension to trust,
    /// so what was actually decoded decides.</summary>
    [Fact]
    public void TwoConvincinglyTextualUnknownsAreText() =>
        Assert.Equal(FileCompareMode.Text,
            FileComparePlan.For(Side("README", looksLikeText: true), Side("LICENCE", looksLikeText: true)));

    [Fact]
    public void OneUnconvincingUnknownMakesThePairBinary() =>
        Assert.Equal(FileCompareMode.Binary,
            FileComparePlan.For(Side("README", looksLikeText: true), Side("blob", looksLikeText: false)));

    /// <summary>
    /// The extension is trusted over the content, so an empty .cs or one full of unusual characters
    /// is still code — the same call the preview pane makes.
    /// </summary>
    [Fact]
    public void AKnownTextExtensionIsTextEvenWhenItDidNotConvince() =>
        Assert.Equal(FileCompareMode.Text,
            FileComparePlan.For(Side("a.cs", looksLikeText: false), Side("b.cs", looksLikeText: false)));

    /// <summary>"Show as text anyway" — nobody is stuck with the automatic answer.</summary>
    [Fact]
    public void ForcingTextWins() =>
        Assert.Equal(FileCompareMode.Text,
            FileComparePlan.For(Side("a.exe"), Side("b.exe"), forceText: true));

    /// <summary>
    /// PreviewClassifier calls an extensionless file text, because for a pane a wrong guess is one
    /// panel of mojibake to dismiss. Here a wrong guess is two panels with differences marked
    /// between them, which reads as a finding — so a name with no extension has to earn it from
    /// what was decoded.
    /// </summary>
    [Fact]
    public void AnExtensionlessNameDoesNotCountAsTextOnItsOwn() =>
        Assert.Equal(FileCompareMode.Binary,
            FileComparePlan.For(Side("blob"), Side("blob2")));
}
