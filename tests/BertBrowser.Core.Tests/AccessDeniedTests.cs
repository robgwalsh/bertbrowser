using System.Runtime.Versioning;
using System.Security.AccessControl;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The one bit that separates a failure an administrator token could fix from every other kind.
/// </summary>
/// <remarks>
/// <para>
/// The theories are the rule; the file tests are the wiring. Both halves matter, and the file tests
/// are where the value is: a predicate that is correct in isolation but never reaches
/// <c>FailedTransfer.AccessDenied</c> is worth nothing, and an over-eager one costs the user a UAC
/// prompt in front of a file that is merely open in Word.
/// </para>
/// <para>
/// The negative cases are deliberately the ones a careless widening would break — a sharing
/// violation and a read-only attribute, which are the two failures that look most like a permission
/// problem from a distance and are neither.
/// </para>
/// </remarks>
[SupportedOSPlatform("windows")]
public sealed class AccessDeniedTests : IDisposable
{
    private readonly string _root;
    private readonly List<string> _denied = [];

    public AccessDeniedTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-denied-{Guid.NewGuid():N}");
        Directory.CreateDirectory(_root);
    }

    public void Dispose()
    {
        // The Deny ACEs come off first, or the fixture cannot be removed and every later run leaves
        // another undeletable one behind in %TEMP%.
        foreach (var folder in _denied)
        {
            try
            {
                Undeny(folder);
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
            }
        }

        foreach (var path in Directory.EnumerateFiles(_root, "*", SearchOption.AllDirectories))
        {
            try
            {
                File.SetAttributes(path, FileAttributes.Normal);
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
            }
        }

        try
        {
            Directory.Delete(_root, recursive: true);
        }
        catch (IOException)
        {
        }
    }

    // --- the rule ---

    [Fact]
    public void UnauthorizedAccessIsAPermissionProblem() =>
        Assert.True(AccessDenied.Caused(new UnauthorizedAccessException("denied")));

    [Fact]
    public void AnIOExceptionCarryingTheAccessDeniedHResultIsOneToo() =>
        Assert.True(AccessDenied.Caused(new IOException("denied", AccessDenied.HResult)));

    [Fact]
    public void TheNotSameDeviceHResultIsNot() =>
        Assert.False(AccessDenied.Caused(new IOException("wrong volume", unchecked((int)0x80070011))));

    [Fact]
    public void ASharingViolationIsNot() =>
        Assert.False(AccessDenied.Caused(new IOException("in use", unchecked((int)0x80070020))));

    [Theory]
    [InlineData(typeof(FileNotFoundException))]
    [InlineData(typeof(DirectoryNotFoundException))]
    [InlineData(typeof(NotSupportedException))]
    [InlineData(typeof(ArgumentException))]
    public void OtherFailuresAreNot(Type kind) =>
        Assert.False(AccessDenied.Caused((Exception)Activator.CreateInstance(kind)!));

    [Fact]
    public void ASecurityExceptionIsNot() =>
        // Excluded on purpose: a CAS-era type the file APIs do not throw. Including it would be
        // speculation, and every false positive here is a UAC prompt that cannot help.
        Assert.False(AccessDenied.Caused(new System.Security.SecurityException("nope")));

    [Fact]
    public void NothingIsNot() => Assert.False(AccessDenied.Caused(null));

    // --- the wiring, through the real executors ---

    [Fact]
    public void ATransferWindowsRefusesIsMarkedAsSuch()
    {
        var source = File_("payload", "src", "a.txt");
        var dest = Dir("dest");

        var planner = new TransferPlanner();
        var executor = new TransferExecutor(
            new FileSystemTransferProbe(), new RefusingCopier());

        var outcome = executor.Execute(planner.Plan([source], dest, TransferVerb.Move));

        var failure = Assert.Single(outcome.Failed);
        Assert.True(failure.AccessDenied);
    }

    [Fact]
    public void ATransferThatFailedForSomeOtherReasonIsNot()
    {
        var source = File_("payload", "src", "a.txt");
        var dest = Dir("dest");

        var planner = new TransferPlanner();
        var executor = new TransferExecutor(
            new FileSystemTransferProbe(), new SulkingCopier());

        var outcome = executor.Execute(planner.Plan([source], dest, TransferVerb.Move));

        var failure = Assert.Single(outcome.Failed);
        Assert.False(failure.AccessDenied);
    }

    [Fact]
    public void ADeleteWindowsRefusesIsMarkedAsSuch()
    {
        var victim = DeniedFile("precious", "locked", "a.txt");

        var outcome = Delete(victim);

        var failure = Assert.Single(outcome.Failed);
        Assert.True(failure.AccessDenied, $"expected a permission failure, got '{failure.Message}'.");
    }

    [Fact]
    public void AFileSomethingElseHasOpenIsNotAPermissionProblem()
    {
        var victim = File_("busy", "open", "a.txt");
        using var hold = new FileStream(victim, FileMode.Open, FileAccess.Read, FileShare.None);

        var outcome = Delete(victim);

        var failure = Assert.Single(outcome.Failed);
        Assert.False(failure.AccessDenied, $"a sharing violation must not ask for a token: '{failure.Message}'.");
    }

    [Fact]
    public void AReadOnlyFileIsNotAPermissionProblemEither()
    {
        // DeleteExecutor.Erase answers UnauthorizedAccessException by clearing the attribute and
        // retrying, so this never reaches the flag at all — it deletes. That order is the reason
        // read-only files do not raise a UAC prompt, and this is what holds it in place.
        var victim = File_("stubborn", "ro", "a.txt");
        File.SetAttributes(victim, FileAttributes.ReadOnly);

        var outcome = Delete(victim);

        Assert.Empty(outcome.Failed);
        Assert.False(File.Exists(victim));
    }

    // --- helpers ---

    private string Dir(params string[] parts)
    {
        var path = P(parts);
        Directory.CreateDirectory(path);
        return path;
    }

    private string File_(string content, params string[] parts)
    {
        var path = P(parts);
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, content);
        return path;
    }

    private string P(params string[] parts) => Path.Combine([_root, .. parts]);

    private DeleteOutcome Delete(string path)
    {
        var planner = new DeletePlanner(new FileSystemDeleteProbe(), [], NoRecycleProbe.Instance);
        var executor = new DeleteExecutor(
            new FileSystemDeleteProbe(), [], stagingRoot: _root, recycleBin: null,
            recycleProbe: NoRecycleProbe.Instance);

        return executor.Execute(planner.Plan([new DeleteSource(path, IsDirectory: false)], DeleteMode.Permanent));
    }

    /// <summary>
    /// A Deny ACE for the account running the test — which needs no privilege at all, since these
    /// are this user's own files. The only way to produce a genuine ERROR_ACCESS_DENIED without
    /// touching a system folder, and what proves the flag survives the whole path from Win32 through
    /// the executor's catch clause.
    /// </summary>
    /// <remarks>
    /// <b>It goes on the folder, not on the file, and that is not arbitrary.</b> Windows lets a
    /// file be deleted when its parent grants <c>FILE_DELETE_CHILD</c>, whatever the file's own DACL
    /// says — so a Deny ACE on the file alone is simply ignored and the delete succeeds. Denying
    /// <see cref="FileSystemRights.DeleteSubdirectoriesAndFiles"/> on the containing folder is what
    /// actually refuses it.
    /// </remarks>
    /// <summary>
    /// A file Windows will refuse to delete: the folder is denied first, so the file <em>inherits</em>
    /// the denial when it is created.
    /// </summary>
    /// <remarks>
    /// The order matters and cost an afternoon. Adding an inheritable Deny ACE to a folder that
    /// already contains the file does not rewrite that file's DACL — it keeps the Full Control it
    /// inherited when it was made, that grant alone is enough to delete it, and the denial is simply
    /// never consulted. Creating the file underneath an already-denied folder is the only version of
    /// this that reproduces what a genuinely protected file looks like.
    /// </remarks>
    private string DeniedFile(string content, params string[] parts)
    {
        var path = P(parts);
        var folder = Path.GetDirectoryName(path)!;
        Directory.CreateDirectory(folder);

        var directory = new DirectoryInfo(folder);
        var security = directory.GetAccessControl();
        security.AddAccessRule(FolderDenial());
        directory.SetAccessControl(security);
        _denied.Add(folder);

        File.WriteAllText(path, content);
        return path;
    }

    private static void Undeny(string folder)
    {
        var directory = new DirectoryInfo(folder);
        var security = directory.GetAccessControl();
        security.RemoveAccessRuleAll(FolderDenial());
        directory.SetAccessControl(security);

        // The children keep the ACE they inherited when they were created; clearing inheritance and
        // handing the folder's own rules back is what makes the fixture removable again.
        foreach (var path in Directory.EnumerateFiles(folder))
        {
            var file = new FileInfo(path);
            var fileSecurity = file.GetAccessControl();
            fileSecurity.SetAccessRuleProtection(isProtected: false, preserveInheritance: false);
            file.SetAccessControl(fileSecurity);
        }
    }

    /// <summary>Delete rights only. Write is deliberately left alone: the fixture has to be able to
    /// create the file inside the folder after the denial is in place.</summary>
    private static FileSystemAccessRule FolderDenial() =>
        new(Environment.UserName,
            FileSystemRights.Delete | FileSystemRights.DeleteSubdirectoriesAndFiles,
            InheritanceFlags.ObjectInherit | InheritanceFlags.ContainerInherit,
            PropagationFlags.None,
            AccessControlType.Deny);
}

/// <summary>A copier that always reports the one failure an administrator token could fix. The
/// seam <c>IFileCopier</c> exists for, used here to land a permission failure deterministically
/// where a real one would need an ACL fixture per test.</summary>
internal sealed class RefusingCopier : IFileCopier
{
    public void Copy(string source, string destination, Action<long, long>? progress, CancellationToken ct) =>
        throw new UnauthorizedAccessException($"Access to the path '{destination}' is denied.");

    public void Move(string source, string destination, Action<long, long>? progress, CancellationToken ct) =>
        throw new UnauthorizedAccessException($"Access to the path '{destination}' is denied.");
}

/// <summary>A copier that fails for a reason no token would fix — the case that must never offer
/// elevation.</summary>
internal sealed class SulkingCopier : IFileCopier
{
    public void Copy(string source, string destination, Action<long, long>? progress, CancellationToken ct) =>
        throw new IOException("The disk is full.", unchecked((int)0x80070070));

    public void Move(string source, string destination, Action<long, long>? progress, CancellationToken ct) =>
        throw new IOException("The disk is full.", unchecked((int)0x80070070));
}
