using BertBrowser.Core.Paths;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The "name (2)" rule. It was copied into the transfer and delete executors and now has a third
/// caller in the new-item dialog, so it is one shared function — and a user who sees
/// "Report (2).txt" from a paste should see the same from a New File.
/// </summary>
public sealed class UniquePathTests
{
    private readonly HashSet<string> _directories = new(StringComparer.OrdinalIgnoreCase);
    private readonly HashSet<string> _files = new(StringComparer.OrdinalIgnoreCase);

    private string Unique(string path, bool isDirectory = false) =>
        UniquePath.For(path, isDirectory, _directories.Contains, _files.Contains);

    // --- the ordinary cases ---

    [Fact]
    public void AFreeNameIsHandedBackUnchanged() =>
        Assert.Equal(@"C:\dir\notes.txt", Unique(@"C:\dir\notes.txt"));

    [Fact]
    public void AFileIsNumberedBeforeItsExtension()
    {
        _files.Add(@"C:\dir\notes.txt");
        Assert.Equal(@"C:\dir\notes (2).txt", Unique(@"C:\dir\notes.txt"));
    }

    [Fact]
    public void AFolderNumbersTheWholeName_BecauseItHasNoExtensionToKeep()
    {
        _directories.Add(@"C:\dir\My.Project");
        Assert.Equal(@"C:\dir\My.Project (2)", Unique(@"C:\dir\My.Project", isDirectory: true));
    }

    [Fact]
    public void TheNumberingSkipsPastNamesThatAreAlsoTaken()
    {
        _files.Add(@"C:\dir\notes.txt");
        _files.Add(@"C:\dir\notes (2).txt");
        _files.Add(@"C:\dir\notes (3).txt");
        Assert.Equal(@"C:\dir\notes (4).txt", Unique(@"C:\dir\notes.txt"));
    }

    // --- files and folders share one namespace ---

    [Fact]
    public void AFolderInTheWayOfAFile_StillCountsAsTaken()
    {
        _directories.Add(@"C:\dir\notes.txt");
        Assert.NotEqual(@"C:\dir\notes.txt", Unique(@"C:\dir\notes.txt"));
    }

    [Fact]
    public void TheNumberGoesByWhatIsBeingPlaced_NotByWhatIsInTheWay()
    {
        // A folder named "notes.txt" blocking a new *file* called "notes.txt" must still give
        // "notes (2).txt". Probe the path instead of being told, and this comes back as
        // "notes.txt (2)".
        _directories.Add(@"C:\dir\notes.txt");
        Assert.Equal(@"C:\dir\notes (2).txt", Unique(@"C:\dir\notes.txt", isDirectory: false));
    }
}
