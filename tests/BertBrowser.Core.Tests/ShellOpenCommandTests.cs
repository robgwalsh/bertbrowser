using BertBrowser.Core.Services.ShellIntegration;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Reading the command Windows would run to open a file, so it can be started with an administrator
/// token instead.
/// </summary>
/// <remarks>
/// Everything here ends in starting a program elevated, so the bias is towards refusing. A command
/// this cannot read confidently must come back null and grey the menu item out — starting an
/// approximation with a token is the one outcome worth avoiding at any cost.
/// </remarks>
public class ShellOpenCommandTests
{
    private const string File = @"C:\Source\app\app.sln";

    /// <summary>Every path any of these tests names as real.</summary>
    private static Func<string, bool> Real(params string[] paths) =>
        path => paths.Contains(path, StringComparer.OrdinalIgnoreCase);

    [Fact]
    public void AQuotedProgramWithAPlaceholderIsTheOrdinaryCase()
    {
        // What .sln actually registers.
        var exe = @"C:\Program Files (x86)\Common Files\Microsoft Shared\MSEnv\VSLauncher.exe";

        var parsed = ShellOpenCommandParser.Parse($"\"{exe}\" \"%1\"", File, Real(exe));

        Assert.NotNull(parsed);
        Assert.Equal(exe, parsed.Executable);
        Assert.Equal($"\"{File}\"", parsed.Arguments);
    }

    [Theory]
    [InlineData("%1")]
    [InlineData("%L")]
    [InlineData("%l")]
    [InlineData("%V")]
    [InlineData("%D")]
    [InlineData("%*")]
    public void EveryPlaceholderTheShellSubstitutesIsSubstituted(string placeholder)
    {
        var exe = @"C:\tools\app.exe";

        var parsed = ShellOpenCommandParser.Parse($"\"{exe}\" \"{placeholder}\"", File, Real(exe));

        Assert.Equal($"\"{File}\"", parsed!.Arguments);
    }

    [Fact]
    public void AnUnquotedProgramWithNoSpacesIsRead()
    {
        var exe = @"C:\Windows\notepad.exe";

        var parsed = ShellOpenCommandParser.Parse($"{exe} %1", File, Real(exe));

        Assert.Equal(exe, parsed!.Executable);
    }

    [Fact]
    public void AnUnquotedProgramWithSpacesIsResolvedTheWayWindowsResolvesIt()
    {
        // "C:\Program" is not the program, however much the first space suggests it. Windows probes
        // each candidate; so does this, longest first.
        var exe = @"C:\Program Files\Vendor App\run.exe";

        var parsed = ShellOpenCommandParser.Parse($"{exe} %1", File, Real(exe));

        Assert.Equal(exe, parsed!.Executable);
        Assert.Equal(File, parsed.Arguments);
    }

    // --- the refusals ---

    [Fact]
    public void ACommandNamingAProgramThatIsNotThereIsRefused() =>
        Assert.Null(ShellOpenCommandParser.Parse(
            @"""C:\gone\app.exe"" ""%1""", File, Real()));

    [Fact]
    public void ACommandThatNeverSaysWhereTheFileGoesIsRefused()
    {
        // Appending the path anyway would hand a program an argument it never asked for, elevated.
        var exe = @"C:\tools\app.exe";

        Assert.Null(ShellOpenCommandParser.Parse($"\"{exe}\"", File, Real(exe)));
    }

    [Fact]
    public void AnUnquotedProgramNoPrefixOfWhichExistsIsRefused() =>
        Assert.Null(ShellOpenCommandParser.Parse(
            @"C:\Program Files\Vendor App\run.exe %1", File, Real(@"C:\something\else.exe")));

    [Fact]
    public void AnUnterminatedQuoteIsRefused() =>
        Assert.Null(ShellOpenCommandParser.Parse(@"""C:\tools\app.exe %1", File, Real()));

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    public void NothingIsRefused(string? command) =>
        Assert.Null(ShellOpenCommandParser.Parse(command, File, Real()));

    [Fact]
    public void AnEnvironmentVariableIsNotMistakenForAPlaceholder()
    {
        // %SystemRoot% is expanded by the caller before it gets here; what must not happen is the
        // parser treating the % pairs around it as somewhere to put the file.
        var exe = @"C:\Windows\system32\app.exe";

        var parsed = ShellOpenCommandParser.Parse($"\"{exe}\" -config %SystemRoot%\\x.ini \"%1\"", File, Real(exe));

        Assert.Equal($"-config C:\\Windows\\x.ini \"{File}\"", parsed!.Arguments.Replace(
            "%SystemRoot%", "C:\\Windows", StringComparison.Ordinal));
    }
}
