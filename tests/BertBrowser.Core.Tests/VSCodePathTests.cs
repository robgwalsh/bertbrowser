using BertBrowser.Core.Services;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The rule that keeps a console window off the screen. <c>code</c> on PATH is a batch shim, and
/// starting a batch file through the shell starts cmd.exe to do it — so the editor arrives with a
/// terminal beside it that looks like the app opened one by mistake. Stepping over the shim is the
/// fix, and the danger in stepping over one is starting the <em>wrong</em> program, which is why
/// nearly every theory here asserts a refusal.
/// </summary>
public class VSCodePathTests
{
    /// <summary>A filesystem containing exactly what it is told to, answering the way
    /// <see cref="ExecutablePath.Resolve"/> does over a fully qualified path.</summary>
    private static Func<string, string?> Existing(params string[] paths)
    {
        var set = new HashSet<string>(paths, StringComparer.OrdinalIgnoreCase);
        return candidate => set.Contains(candidate) ? candidate : null;
    }

    private const string Bin = @"C:\Users\Rob\AppData\Local\Programs\Microsoft VS Code\bin\code.cmd";
    private const string Editor = @"C:\Users\Rob\AppData\Local\Programs\Microsoft VS Code\Code.exe";

    [Fact]
    public void TheShimBecomesTheEditorAboveIt() =>
        Assert.Equal(Editor, VSCodePath.BehindLauncher(Bin, Existing(Editor)),
            StringComparer.OrdinalIgnoreCase);

    /// <summary>The case the stem does not name: the launcher is <c>code-insiders.cmd</c> and the
    /// program beside it is <c>Code - Insiders.exe</c>.</summary>
    [Fact]
    public void AnInsidersShimFindsTheNameItDoesNotShare()
    {
        const string shim = @"C:\VS Code Insiders\bin\code-insiders.cmd";
        const string editor = @"C:\VS Code Insiders\Code - Insiders.exe";

        Assert.Equal(editor, VSCodePath.BehindLauncher(shim, Existing(editor)),
            StringComparer.OrdinalIgnoreCase);
    }

    /// <summary>Preference order, not "the first thing found": an install holding both is the
    /// stable one with an Insiders build unpacked beside it, and stable is what <c>code</c> means.
    /// </summary>
    [Fact]
    public void StableWinsOverInsidersInOneRoot()
    {
        Assert.Equal(
            Editor,
            VSCodePath.BehindLauncher(Bin, Existing(
                Editor,
                @"C:\Users\Rob\AppData\Local\Programs\Microsoft VS Code\Code - Insiders.exe")),
            StringComparer.OrdinalIgnoreCase);
    }

    /// <summary>Nothing recognisable above the shim is a null, never a guess — the caller then
    /// starts the shim it already had, console window and all, which opens the right editor.
    /// </summary>
    [Fact]
    public void AnUnrecognisedInstallIsRefused() =>
        Assert.Null(VSCodePath.BehindLauncher(Bin, Existing(
            @"C:\Users\Rob\AppData\Local\Programs\Microsoft VS Code\unins000.exe")));

    /// <summary>The <c>bin</c> requirement is what stops this being "run the .exe next to any
    /// .cmd". A shim sitting beside its own program would otherwise send us a level up into a
    /// folder full of other people's programs.</summary>
    [Fact]
    public void AShimOutsideABinFolderIsRefused() =>
        Assert.Null(VSCodePath.BehindLauncher(@"C:\tools\code.cmd", Existing(@"C:\Code.exe")));

    [Fact]
    public void AnExecutableIsNotAShimAndIsLeftAlone() =>
        Assert.Null(VSCodePath.BehindLauncher(Editor, Existing(Editor)));

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    public void NothingResolvedIsNothingToStepOver(string? launcher) =>
        Assert.Null(VSCodePath.BehindLauncher(launcher, Existing(Editor)));

    /// <summary>A <c>.bat</c> is the same shape and the same trap; VSCodium ships one of these
    /// layouts too.</summary>
    [Fact]
    public void ABatShimIsTakenTheSameWay()
    {
        const string shim = @"C:\Program Files\VSCodium\bin\codium.bat";
        const string editor = @"C:\Program Files\VSCodium\VSCodium.exe";

        Assert.Equal(editor, VSCodePath.BehindLauncher(shim, Existing(editor)),
            StringComparer.OrdinalIgnoreCase);
    }

    /// <summary>An install laid out at a drive root still resolves: the root above the
    /// <c>bin</c> is <c>C:\</c>, which is a folder like any other.</summary>
    [Fact]
    public void AShimAtADriveRootStillFindsTheEditor() =>
        Assert.Equal(@"C:\Code.exe",
            VSCodePath.BehindLauncher(@"C:\bin\code.cmd", Existing(@"C:\Code.exe")),
            StringComparer.OrdinalIgnoreCase);
}
