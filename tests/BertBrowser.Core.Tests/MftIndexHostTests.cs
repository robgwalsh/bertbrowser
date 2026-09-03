using BertBrowser.Core.Ipc;
using BertBrowser.Core.Services.Changes;
using BertBrowser.Core.Services.Mft;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The elevated half. What it will accept is the security surface of the whole split, so most of
/// this file is about what it refuses to do.
/// </summary>
public class MftIndexHostTests
{
    private static readonly TimeSpan Patience = TimeSpan.FromSeconds(5);

    /// <summary>Runs a host on its own thread against a peer the test drives as the app.</summary>
    private static (Thread Thread, ProtocolPeer App) Hosted(ControllableIndexService index)
    {
        var (hostEnd, appEnd) = DuplexPair.Create();
        var host = new MftIndexHost(index, hostEnd);
        var thread = new Thread(() => { try { host.Run(); } catch (Exception) { /* asserted via state */ } })
        {
            IsBackground = true,
            Name = "test index host",
        };
        thread.Start();
        return (thread, new ProtocolPeer(appEnd));
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
    public void GreetsWithItsVersionAndAnnouncesItIsReady()
    {
        var (thread, app) = Hosted(new ControllableIndexService());

        var hello = app.Receive()!.Value;
        Assert.Equal(IndexVerb.Hello, hello.Verb);
        Assert.Equal(IndexProtocol.ProtocolVersion, IndexProtocol.VersionOf(hello));
        Assert.Equal(IndexVerb.Ready, app.Receive()!.Value.Verb);

        app.Close();
        thread.Join(Patience);
    }

    [Fact]
    public void StartsIndexingWhenAsked()
    {
        var index = new ControllableIndexService();
        var (thread, app) = Hosted(index);
        app.Receive();
        app.Receive();

        app.Send(IndexVerb.Start);

        Eventually(() => index.Starts == 1, "Start should start the indexer");
        app.Close();
        thread.Join(Patience);
    }

    [Fact]
    public void AppliesTheRecordingPolicyItIsSent()
    {
        var index = new ControllableIndexService();
        var (thread, app) = Hosted(index);
        app.Receive();
        app.Receive();
        Assert.False(index.ChangeLog.Enabled);

        app.Send(IndexVerb.Record, "24");
        Eventually(() => index.ChangeLog == ChangeLogPolicy.FromHours(24), "Record 24 should turn recording on for a day");

        app.Send(IndexVerb.Record, "0");
        Eventually(() => !index.ChangeLog.Enabled, "Record 0 should turn recording off");

        app.Close();
        thread.Join(Patience);
    }

    /// <summary>A second Start must not run a second set of volume threads.</summary>
    [Fact]
    public void StartsAtMostOnce()
    {
        var index = new ControllableIndexService();
        var (thread, app) = Hosted(index);

        app.Send(IndexVerb.Start);
        app.Send(IndexVerb.Start);
        app.Send(IndexVerb.Start);
        Eventually(() => index.Starts == 1, "the first Start should take");
        app.Send(IndexVerb.Ping);
        Assert.Equal(IndexVerb.Pong, app.ReceiveOneOf(IndexVerb.Pong)!.Value.Verb);

        Assert.Equal(1, index.Starts);
        app.Close();
        thread.Join(Patience);
    }

    [Fact]
    public void ReportsBuildingAndCompletionAsItHappens()
    {
        var index = new ControllableIndexService();
        var (thread, app) = Hosted(index);
        app.Send(IndexVerb.Start);
        Eventually(() => index.Starts == 1, "precondition: started");

        index.BeginBuilding("C");
        var building = app.ReceiveOneOf(IndexVerb.Building)!.Value;
        Assert.Equal("C", building.Argument);

        index.FinishBuilding("C", @"C:\");
        Assert.Equal(@"C:\", app.ReceiveOneOf(IndexVerb.Complete)!.Value.Argument);

        app.Close();
        thread.Join(Patience);
    }

    /// <summary>A volume that fails reports Idle without a Complete — that is a real outcome.</summary>
    [Fact]
    public void ReportsIdleForAVolumeThatNeverCompletes()
    {
        var index = new ControllableIndexService();
        var (thread, app) = Hosted(index);
        app.Send(IndexVerb.Start);
        Eventually(() => index.Starts == 1, "precondition: started");

        index.BeginBuilding("C");
        Assert.Equal("C", app.ReceiveOneOf(IndexVerb.Building)!.Value.Argument);

        index.AbandonBuilding("C");
        var idle = app.ReceiveOneOf(IndexVerb.Idle, IndexVerb.Complete)!.Value;
        Assert.Equal(IndexVerb.Idle, idle.Verb);
        Assert.Equal("C", idle.Argument);

        app.Close();
        thread.Join(Patience);
    }

    /// <summary>Each transition is reported once, not on every status change.</summary>
    [Fact]
    public void DoesNotRepeatABuildingItAlreadyReported()
    {
        var index = new ControllableIndexService();
        var (thread, app) = Hosted(index);
        app.Send(IndexVerb.Start);
        Eventually(() => index.Starts == 1, "precondition: started");

        index.BeginBuilding("C");
        index.BeginBuilding("C");
        index.BeginBuilding("D");
        Assert.Equal("C", app.ReceiveOneOf(IndexVerb.Building)!.Value.Argument);
        Assert.Equal("D", app.ReceiveOneOf(IndexVerb.Building)!.Value.Argument);

        app.Close();
        thread.Join(Patience);
    }

    [Fact]
    public void AnswersAPing()
    {
        var (thread, app) = Hosted(new ControllableIndexService());

        app.Send(IndexVerb.Ping);

        Assert.Equal(IndexVerb.Pong, app.ReceiveOneOf(IndexVerb.Pong)!.Value.Verb);
        app.Close();
        thread.Join(Patience);
    }

    [Fact]
    public void ShutdownEndsTheRun()
    {
        var (thread, app) = Hosted(new ControllableIndexService());

        app.Send(IndexVerb.Shutdown);

        Assert.True(thread.Join(Patience), "Shutdown should return from Run");
    }

    /// <summary>
    /// The primary shutdown signal, and the one that works when the app crashes rather than exits:
    /// the kernel breaks the pipe and the read returns nothing.
    /// </summary>
    [Fact]
    public void LosingTheAppEndsTheRun()
    {
        var (thread, app) = Hosted(new ControllableIndexService());

        app.Close();

        Assert.True(thread.Join(Patience), "a broken pipe should return from Run");
    }

    [Fact]
    public void IgnoresAMalformedMessageAndKeepsRunning()
    {
        var index = new ControllableIndexService();
        var (thread, app) = Hosted(index);

        app.SendRaw("garbage");
        app.SendRaw("IDX\tReindex\tC:\\Windows");
        app.SendRaw("IDX\tStart\tC:\\Windows");
        app.Send(IndexVerb.Start);

        Eventually(() => index.Starts == 1, "the session should survive malformed lines");
        app.Close();
        thread.Join(Patience);
    }

    /// <summary>
    /// The property the whole split rests on: nothing a medium-integrity peer can say to this
    /// process names a file, a folder or a program.
    /// </summary>
    [Fact]
    public void AVerbCarryingAPathIsNotEvenParsed()
    {
        foreach (var line in new[]
                 {
                     "IDX\tStart\tC:\\Windows\\System32",
                     "IDX\tShutdown\t\\\\server\\share",
                     "IDX\tPing\tC:\\",
                     "IDX\tHello\tC:\\",
                 })
        {
            Assert.False(IndexProtocol.TryParse(line, out _), line);
        }
    }

    [Fact]
    public void RefusesAnAppSpeakingAnotherProtocolVersion()
    {
        var index = new ControllableIndexService();
        var (thread, app) = Hosted(index);

        app.Send(IndexVerb.Hello, (IndexProtocol.ProtocolVersion + 1).ToString());

        Assert.Equal(IndexVerb.Fatal, app.ReceiveOneOf(IndexVerb.Fatal)!.Value.Verb);
        Assert.True(thread.Join(Patience), "a version mismatch should end the run");
        Assert.Equal(0, index.Starts);
    }

    [Fact]
    public void AcceptsAMatchingHelloAndCarriesOn()
    {
        var index = new ControllableIndexService();
        var (thread, app) = Hosted(index);

        app.Send(IndexVerb.Hello, IndexProtocol.ProtocolVersion.ToString());
        app.Send(IndexVerb.Start);

        Eventually(() => index.Starts == 1, "a matching version should not end the run");
        app.Close();
        thread.Join(Patience);
    }

    /// <summary>
    /// The app cannot see this process's exceptions, so an indexer that cannot run has to say so on
    /// the wire — otherwise it looks exactly like one that is merely slow, forever.
    /// </summary>
    [Fact]
    public void AnIndexerThatCannotStartSaysSoBeforeDying()
    {
        var index = new ControllableIndexService
        {
            StartThrows = new InvalidOperationException("The database is locked by another process."),
        };
        var (thread, app) = Hosted(index);

        app.Send(IndexVerb.Start);

        var fatal = app.ReceiveOneOf(IndexVerb.Fatal)!.Value;
        Assert.Equal("The database is locked by another process.", fatal.Argument);
        Assert.True(thread.Join(Patience), "an indexer that cannot start should end the run");
    }

    /// <summary>A multi-line exception message must not become two messages on the wire.</summary>
    [Fact]
    public void AMultiLineFailureIsFlattenedIntoOneMessage()
    {
        var index = new ControllableIndexService
        {
            StartThrows = new InvalidOperationException("first line\r\nsecond line\tthird"),
        };
        var (thread, app) = Hosted(index);

        app.Send(IndexVerb.Start);

        var fatal = app.ReceiveOneOf(IndexVerb.Fatal)!.Value;
        Assert.DoesNotContain('\n', fatal.Argument);
        Assert.DoesNotContain('\r', fatal.Argument);
        Assert.Contains("first line", fatal.Argument);
        thread.Join(Patience);
    }
}
