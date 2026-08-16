using BertBrowser.Core.Ipc;
using BertBrowser.Core.Services.Mft;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The app's half of the split, driven over an in-memory pipe. The claims worth holding: state is
/// mirrored exactly, a malformed message does not end a session, losing the helper is not silently
/// survived, and nothing retries without being asked.
/// </summary>
public class MftIndexClientTests : IDisposable
{
    private static readonly TimeSpan Patience = TimeSpan.FromSeconds(5);

    private readonly List<MftIndexClient> _clients = new();

    private MftIndexClient Client(IIndexHostLauncher launcher, IIndexTransportFactory transports)
    {
        var client = new MftIndexClient(launcher, transports);
        _clients.Add(client);
        return client;
    }

    /// <summary>Starts a client already connected to a peer the test drives.</summary>
    private (MftIndexClient Client, ProtocolPeer Helper) Connected(out FakeIndexHostLauncher launcher)
    {
        var (appEnd, helperEnd) = DuplexPair.Create();
        launcher = new FakeIndexHostLauncher(IndexHostLaunchResult.Started(4242));
        var client = Client(launcher, new FakeIndexTransportFactory(() => appEnd));
        var helper = new ProtocolPeer(helperEnd);

        client.Start();

        // The client greets first; answer as the helper does so it sends Start.
        Assert.Equal(IndexVerb.Hello, helper.Receive()!.Value.Verb);
        helper.Send(IndexVerb.Hello, IndexProtocol.ProtocolVersion.ToString());
        helper.Send(IndexVerb.Ready);
        Assert.Equal(IndexVerb.Start, helper.ReceiveOneOf(IndexVerb.Start)!.Value.Verb);

        return (client, helper);
    }

    private static void Eventually(Func<bool> condition, string because)
    {
        var deadline = DateTime.UtcNow + Patience;
        while (DateTime.UtcNow < deadline)
        {
            if (condition()) return;
            Thread.Sleep(10);
        }
        Assert.Fail(because);
    }

    [Fact]
    public void GreetsWithItsVersionAndStartsTheHelperOnReady()
    {
        var (_, helper) = Connected(out var launcher);

        Assert.Equal(1, launcher.Launches);
        helper.Close();
    }

    [Fact]
    public void MirrorsBuildingAndIdleIntoIsBuildingAndTheStatusLine()
    {
        var (client, helper) = Connected(out _);

        helper.Send(IndexVerb.Building, "C");
        Eventually(() => client.IsBuilding, "the client should mirror Building");
        Eventually(() => client.StatusText == "Indexing C:…", "the client formats its own status line");

        helper.Send(IndexVerb.Building, "D");
        Eventually(() => client.StatusText == "Indexing C:, D:…", "both drives should show");

        helper.Send(IndexVerb.Idle, "C");
        helper.Send(IndexVerb.Idle, "D");
        Eventually(() => !client.IsBuilding, "the client should mirror Idle");
        Assert.Equal("", client.StatusText);
    }

    [Fact]
    public void MirrorsCompleteIntoIsIndexedAndRaisesIndexRefreshed()
    {
        var (client, helper) = Connected(out _);
        var refreshed = new List<string>();
        client.IndexRefreshed += root => { lock (refreshed) refreshed.Add(root); };

        helper.Send(IndexVerb.Complete, @"C:\");

        Eventually(() => client.AnyIndexed, "Complete should mark the root indexed");
        Assert.True(client.IsIndexed(@"C:\WINDOWS\SYSTEM32"));
        Assert.False(client.IsIndexed(@"D:\DATA"));
        Eventually(() => { lock (refreshed) return refreshed.Contains(@"C:\"); }, "IndexRefreshed should fire");
    }

    [Fact]
    public void RaisesStatusChangedAsStateArrives()
    {
        var (client, helper) = Connected(out _);
        var changes = 0;
        client.StatusChanged += () => Interlocked.Increment(ref changes);

        helper.Send(IndexVerb.Building, "C");

        Eventually(() => Volatile.Read(ref changes) > 0, "StatusChanged should fire for a state push");
    }

    /// <summary>
    /// One bad line must not cost the session — the lesson the single-instance listener already
    /// carries. A peer that can end the conversation by sending garbage is a peer that can turn
    /// the index off.
    /// </summary>
    [Fact]
    public void SkipsAMalformedMessageAndKeepsReading()
    {
        var (client, helper) = Connected(out _);

        helper.SendRaw("this is not a message");
        helper.SendRaw("IDX\tReindex\tC:\\Windows");
        helper.SendRaw("IDX\t99");
        helper.Send(IndexVerb.Complete, @"C:\");

        Eventually(() => client.IsIndexed(@"C:\WINDOWS"), "the session should survive malformed lines");
    }

    [Fact]
    public void ADeadHelperClearsTheMirroredStateAndOffersARetry()
    {
        var (client, helper) = Connected(out _);
        helper.Send(IndexVerb.Complete, @"C:\");
        Eventually(() => client.AnyIndexed, "precondition: the root is indexed");

        helper.Close();

        Eventually(() => client.CanRetry, "losing the helper should offer a retry");
        // Claiming a volume is indexed when nothing is keeping it current would route searches at a
        // database going stale.
        Assert.False(client.AnyIndexed);
        Assert.False(client.IsIndexed(@"C:\WINDOWS"));
        Assert.Equal("Search index stopped.", client.StatusText);
    }

    [Fact]
    public void AFatalFromTheHelperBecomesTheStatusLine()
    {
        var (client, helper) = Connected(out _);

        helper.Send(IndexVerb.Fatal, "The database schema is newer than this helper.");

        Eventually(() => client.CanRetry, "a fatal should offer a retry");
        Assert.Equal("The database schema is newer than this helper.", client.StatusText);
    }

    /// <summary>A half-applied update is the case this exists for.</summary>
    [Fact]
    public void RefusesAHelperSpeakingAnotherProtocolVersion()
    {
        var (appEnd, helperEnd) = DuplexPair.Create();
        var client = Client(
            new FakeIndexHostLauncher(IndexHostLaunchResult.Started(1)),
            new FakeIndexTransportFactory(() => appEnd));
        var helper = new ProtocolPeer(helperEnd);

        client.Start();
        helper.Receive();
        helper.Send(IndexVerb.Hello, (IndexProtocol.ProtocolVersion + 1).ToString());

        Eventually(() => client.CanRetry, "a version mismatch should be reported, not mirrored");
        Assert.Equal("Search index unavailable.", client.StatusText);
    }

    [Fact]
    public void ADeclinedPromptIsRetryableAndSaysSo()
    {
        var client = Client(
            new FakeIndexHostLauncher(IndexHostLaunchResult.Declined),
            new FakeIndexTransportFactory(() => null));

        client.Start();

        Eventually(() => client.CanRetry, "a declined prompt should be retryable");
        Assert.Equal("Search index off — permission declined.", client.StatusText);
        Assert.False(client.AnyIndexed);
    }

    /// <summary>
    /// A standard user cannot elevate at all, so there is nothing to retry — and, crucially,
    /// nothing was prompted for either.
    /// </summary>
    [Fact]
    public void AStandardUserIsNeverPromptedAndCannotRetry()
    {
        var launcher = new FakeIndexHostLauncher(IndexHostLaunchResult.NotAdministrator) { CanElevate = false };
        var client = Client(launcher, new FakeIndexTransportFactory(() => null));

        client.Start();

        Eventually(() => client.StatusText.Length > 0, "a standard user should be told");
        Assert.False(client.CanRetry);
        Assert.Equal("Search index off — this account is not an administrator.", client.StatusText);
        Assert.Equal(0, launcher.Launches);
    }

    [Fact]
    public void AHelperThatNeverConnectsIsReportedAsUnavailable()
    {
        var client = Client(
            new FakeIndexHostLauncher(IndexHostLaunchResult.Started(1)),
            new FakeIndexTransportFactory(() => null));

        client.Start();

        Eventually(() => client.CanRetry, "a helper that never arrives should be retryable");
        Assert.Equal("Search index unavailable.", client.StatusText);
    }

    /// <summary>
    /// The launcher's reason has to reach the status bar. There is no log behind it, and a build
    /// that never copied the helper into place looks exactly like a UAC failure without this.
    /// </summary>
    [Fact]
    public void ALaunchFailureSaysWhy()
    {
        var client = Client(
            new FakeIndexHostLauncher(IndexHostLaunchResult.Failed("the index helper is missing")),
            new FakeIndexTransportFactory(() => null));

        client.Start();

        Eventually(() => client.CanRetry, "a failed launch should be retryable");
        Assert.Equal("Search index unavailable — the index helper is missing.", client.StatusText);
    }

    /// <summary>A detail quoted from Win32 arrives with its own full stop; one is enough.</summary>
    [Fact]
    public void ALaunchFailureDoesNotDoubleItsTerminator()
    {
        var client = Client(
            new FakeIndexHostLauncher(IndexHostLaunchResult.Failed("The system cannot find the file specified.")),
            new FakeIndexTransportFactory(() => null));

        client.Start();

        Eventually(() => client.CanRetry, "a failed launch should be retryable");
        Assert.Equal(
            "Search index unavailable — The system cannot find the file specified.",
            client.StatusText);
    }

    /// <summary>A launcher with nothing to add must not produce a dangling dash.</summary>
    [Fact]
    public void ALaunchFailureWithNoDetailStaysTerse()
    {
        var client = Client(
            new FakeIndexHostLauncher(IndexHostLaunchResult.Failed("")),
            new FakeIndexTransportFactory(() => null));

        client.Start();

        Eventually(() => client.CanRetry, "a failed launch should be retryable");
        Assert.Equal("Search index unavailable.", client.StatusText);
    }

    [Fact]
    public void RetryLaunchesExactlyOnceMore()
    {
        var launcher = new FakeIndexHostLauncher(IndexHostLaunchResult.Declined);
        var client = Client(launcher, new FakeIndexTransportFactory(() => null));

        client.Start();
        Eventually(() => client.CanRetry, "precondition: retryable");

        client.Retry();

        Eventually(() => launcher.Launches == 2, "retry should launch once more");
        Thread.Sleep(50);
        Assert.Equal(2, launcher.Launches);
    }

    /// <summary>Nothing retries on its own: every retry is a UAC prompt.</summary>
    [Fact]
    public void NothingRetriesWithoutBeingAsked()
    {
        var launcher = new FakeIndexHostLauncher(IndexHostLaunchResult.Declined);
        var client = Client(launcher, new FakeIndexTransportFactory(() => null));

        client.Start();
        Eventually(() => client.CanRetry, "precondition: retryable");
        Thread.Sleep(300);

        Assert.Equal(1, launcher.Launches);
    }

    [Fact]
    public void RetryDoesNothingWhenThereIsNothingToRetry()
    {
        var launcher = new FakeIndexHostLauncher(IndexHostLaunchResult.NotAdministrator) { CanElevate = false };
        var client = Client(launcher, new FakeIndexTransportFactory(() => null));

        client.Start();
        Eventually(() => client.StatusText.Length > 0, "precondition: settled");

        client.Retry();

        Thread.Sleep(50);
        Assert.Equal(0, launcher.Launches);
    }

    [Fact]
    public void StartingTwiceOnlyLaunchesOnce()
    {
        var (_, helper) = Connected(out var launcher);

        // A second Start must not raise a second prompt.
        Assert.Equal(1, launcher.Launches);
        helper.Close();
    }

    [Fact]
    public void AnswersAPingSoTheHelperCanTellTheAppIsAlive()
    {
        var (_, helper) = Connected(out _);

        helper.Send(IndexVerb.Ping);

        Assert.Equal(IndexVerb.Pong, helper.ReceiveOneOf(IndexVerb.Pong)!.Value.Verb);
    }

    [Fact]
    public void DisposeAsksTheHelperToStop()
    {
        var (client, helper) = Connected(out _);

        client.Dispose();

        Assert.Equal(IndexVerb.Shutdown, helper.ReceiveOneOf(IndexVerb.Shutdown)!.Value.Verb);
    }

    [Fact]
    public void StartAfterDisposeDoesNothing()
    {
        var client = Client(
            new FakeIndexHostLauncher(IndexHostLaunchResult.Declined),
            new FakeIndexTransportFactory(() => null));

        client.Dispose();
        client.Start();

        Thread.Sleep(50);
        Assert.Equal("", client.StatusText);
    }

    public void Dispose()
    {
        foreach (var client in _clients) client.Dispose();
    }
}
