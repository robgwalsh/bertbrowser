using System.Text;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Archives;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Encrypted containers, against archives 7-Zip really produced.
/// </summary>
/// <remarks>
/// The two cases must look different to the user and so must look different here: a zip with its
/// headers in the clear lists in full and only refuses to open its entries, while a 7z with its
/// headers encrypted has nothing to list at all. SharpCompress 1.0.0 reports no encryption
/// <em>scope</em>, so both are derived from what actually happens when the container is opened —
/// which is why they are worth pinning against real files rather than reasoning about.
/// </remarks>
public class EncryptedArchiveTests : IDisposable
{
    private readonly string _root = Path.Combine(
        Path.GetTempPath(), $"bertbrowser-enc-{Guid.NewGuid():N}");

    private readonly SharpCompressArchiveReader _reader = new();

    public EncryptedArchiveTests() => Directory.CreateDirectory(_root);

    public void Dispose()
    {
        try { Directory.Delete(_root, recursive: true); } catch { /* best effort */ }
    }

    private string Fixture(string name, string base64) =>
        ArchiveFixtures.WriteTo(Path.Combine(_root, name), base64);

    /// <summary>
    /// Names, sizes and dates are in the clear, so hiding them would be pretending. The lock is on
    /// the entries, and that is what the banner says.
    /// </summary>
    [Fact]
    public void AZipWithEncryptedContentsStillListsInFull()
    {
        var path = Fixture("enc.zip", ArchiveFixtures.EncryptedZip);

        var index = _reader.Read(path, password: null);

        Assert.True(index.Ok, index.Error);
        Assert.Equal(2, index.FileCount);
        Assert.Equal(["notes.txt", "secret.txt"], index.Children("")!.Select(n => n.Name).ToArray());
        Assert.True(index.Capabilities.IsEncrypted);
        Assert.All(index.Children("")!, n => Assert.True(n.IsEncrypted));
    }

    [Fact]
    public void ReadingAnEncryptedEntryWithoutThePasswordFailsRatherThanReturningRubbish()
    {
        var path = Fixture("enc.zip", ArchiveFixtures.EncryptedZip);

        Assert.Null(_reader.ReadEntry(path, "secret.txt", 1024, password: null));
    }

    [Fact]
    public void ThePasswordUnlocksTheContents()
    {
        var path = Fixture("enc.zip", ArchiveFixtures.EncryptedZip);

        var bytes = _reader.ReadEntry(path, "secret.txt", 1024, ArchiveFixtures.ZipPassword);

        Assert.NotNull(bytes);
        Assert.Equal("classified", Encoding.UTF8.GetString(bytes!));
    }

    /// <summary>
    /// Nothing can be listed, so the banner is the whole content. This is the arm that must not be
    /// reported as "damaged": there is something the user can do about it.
    /// </summary>
    [Fact]
    public void A7zWithEncryptedHeadersReportsThatAPasswordIsNeeded()
    {
        var path = Fixture("enchdr.7z", ArchiveFixtures.HeaderEncryptedSevenZip);

        var index = _reader.Read(path, password: null);

        Assert.False(index.Ok);
        Assert.Equal(ArchiveFailure.PasswordRequired, index.Failure);
        Assert.Contains("password", index.Error!, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void TheRightPasswordListsAHeaderEncrypted7z()
    {
        var path = Fixture("enchdr.7z", ArchiveFixtures.HeaderEncryptedSevenZip);

        var index = _reader.Read(path, ArchiveFixtures.SevenZipPassword);

        Assert.True(index.Ok, index.Error);
        Assert.Equal(["notes.txt", "secret.txt"], index.Children("")!.Select(n => n.Name).ToArray());
    }

    /// <summary>
    /// A wrong password is its own message, and it must arrive as a message — the exception it
    /// comes from is SharpCompress's own CryptographicException, which shadows the framework type
    /// of the same name. Catch the wrong one and this escapes unhandled out of a Task.Run.
    /// </summary>
    [Fact]
    public void AWrongPasswordIsReportedRatherThanThrown()
    {
        var path = Fixture("enchdr.7z", ArchiveFixtures.HeaderEncryptedSevenZip);

        var index = _reader.Read(path, "not-the-password");

        Assert.False(index.Ok);
        Assert.Equal(ArchiveFailure.PasswordRequired, index.Failure);
        Assert.Contains("did not work", index.Error!);
    }

    /// <summary>
    /// 7z is read-only here, so an ordinary one is the only evidence the format is handled at all —
    /// nothing in the dependency graph can write one to round-trip against.
    /// </summary>
    [Fact]
    public void AnOrdinary7zReadsAndItsEntriesOpen()
    {
        var path = Fixture("plain.7z", ArchiveFixtures.PlainSevenZip);

        var index = _reader.Read(path, password: null);

        Assert.True(index.Ok, index.Error);
        Assert.Equal(2, index.FileCount);

        var bytes = _reader.ReadEntry(path, "secret.txt", 1024, password: null);
        Assert.Equal("classified", Encoding.UTF8.GetString(bytes!));
    }

    /// <summary>
    /// The listing layer turns a locked container into its own exception type, so the one caller
    /// that can offer to unlock can tell it apart — while everything catching IOException, which is
    /// its base, keeps working unchanged.
    /// </summary>
    [Fact]
    public void ListingALockedArchiveThrowsSomethingTheBannerCanActOn()
    {
        var path = Fixture("enchdr.7z", ArchiveFixtures.HeaderEncryptedSevenZip);
        var service = new ArchiveAwareFileSystemService(
            new FileSystemService(), new SharpCompressArchiveReader());

        var ex = Assert.Throws<ArchiveLockedException>(() => service.ListDirectory(path));

        Assert.Equal(path, ex.ArchiveFile);
        Assert.IsAssignableFrom<IOException>(ex);
    }

    /// <summary>Once a password is known, the same listing simply works.</summary>
    [Fact]
    public void AKnownPasswordMakesTheListingSucceed()
    {
        var path = Fixture("enchdr.7z", ArchiveFixtures.HeaderEncryptedSevenZip);
        var service = new ArchiveAwareFileSystemService(
            new FileSystemService(), new SharpCompressArchiveReader(),
            new FixedPassword(ArchiveFixtures.SevenZipPassword));

        var rows = service.ListDirectory(path);

        Assert.Equal(["notes.txt", "secret.txt"], rows.Select(r => r.Name).ToArray());
    }

    private sealed class FixedPassword(string password) : IArchivePasswords
    {
        public string? For(string archiveFile) => password;
    }
}
