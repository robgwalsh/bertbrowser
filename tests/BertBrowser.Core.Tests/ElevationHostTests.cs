using BertBrowser.Core.Ipc;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Elevation;
using BertBrowser.Core.Services.NewItem;
using BertBrowser.Core.Services.Rename;
using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The elevated end, driven over a <see cref="DuplexPair"/> against real files — everything the real
/// helper does except holding a token, which is the one part no test can have.
/// </summary>
/// <remarks>
/// The tests that matter most are the refusals. This is the only class in the product that takes
/// attacker-shaped input and acts on it with administrator rights, so "it does the operation" is the
/// easy half; "it does nothing else, and only once" is the half worth pinning down.
/// </remarks>
public sealed class ElevationHostTests : IDisposable
{
    private static readonly TimeSpan Patience = TimeSpan.FromSeconds(5);

    private readonly string _root;

    public ElevationHostTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-elev-{Guid.NewGuid():N}");
        Directory.CreateDirectory(_root);
    }

    public void Dispose()
    {
        try
        {
            Directory.Delete(_root, recursive: true);
        }
        catch (IOException)
        {
        }
    }

    // --- the greeting ---

    [Fact]
    public void ItGreetsAndSaysItIsReady()
    {
        var (host, app) = Hosted();

        Assert.Equal(ElevationVerb.Hello, app.Receive()!.Value.Verb);
        Assert.Equal(ElevationVerb.Ready, app.Receive()!.Value.Verb);

        app.Close();
        host.Join(Patience);
    }

    [Fact]
    public void ADifferentProtocolVersionEndsTheSession()
    {
        var (host, app) = Hosted();
        Drain(app, ElevationVerb.Ready);

        app.Send(ElevationVerb.Hello, "99");

        Assert.Equal(ElevationVerb.Fatal, app.Receive()!.Value.Verb);
        Assert.True(host.Join(Patience));
    }

    [Fact]
    public void AMalformedLineDoesNotEndTheSession()
    {
        var (host, app) = Hosted();
        Drain(app, ElevationVerb.Ready);

        app.SendRaw("this is not a message");
        app.Request(Header(ElevationOperation.Rename), RenameItem("a.txt", "b.txt"));

        Assert.Contains(app.ReceiveAll(), m => m.Verb == ElevationVerb.End);
        Assert.True(host.Join(Patience));
    }

    // --- one request, and only one ---

    [Fact]
    public void ItServesOneRequestAndThenStops()
    {
        var (host, app) = Hosted();
        Drain(app, ElevationVerb.Ready);
        File_("first", "a.txt");
        File_("second", "c.txt");

        app.Request(Header(ElevationOperation.Rename), RenameItem("a.txt", "b.txt"));
        // A second request, sent the moment the first is accepted. The helper is one-shot by
        // construction, so this must reach nothing at all.
        app.Request(Header(ElevationOperation.Rename), RenameItem("c.txt", "d.txt"));

        app.ReceiveAll();
        Assert.True(host.Join(Patience));

        Assert.True(File.Exists(P("b.txt")));
        Assert.True(File.Exists(P("c.txt")));
        Assert.False(File.Exists(P("d.txt")));
    }

    [Fact]
    public void AnItemArrivingAfterGoIsNotAcceptedIntoTheRequest()
    {
        // The structural half of "one prompt, one plan": a peer that somehow owned the pipe still
        // cannot add a path to an operation that is already running.
        var (host, app) = Hosted();
        Drain(app, ElevationVerb.Ready);
        File_("one", "a.txt");
        File_("two", "c.txt");

        app.Request(Header(ElevationOperation.Rename), RenameItem("a.txt", "b.txt"));
        app.Send(ElevationVerb.Item, RenameItem("c.txt", "d.txt"));

        app.ReceiveAll();
        host.Join(Patience);

        Assert.True(File.Exists(P("c.txt")));
        Assert.False(File.Exists(P("d.txt")));
    }

    [Fact]
    public void ASecondHeaderIsRefused()
    {
        // The state machine's half of "one request per process", asserted directly rather than
        // through its effect on disk: after Go the pipe is closed anyway, so a test that only looked
        // at the filesystem would pass whatever this switch did.
        var (host, app) = Hosted();
        Drain(app, ElevationVerb.Ready);

        app.Send(ElevationVerb.Begin, Header(ElevationOperation.Rename));
        app.Send(ElevationVerb.Begin, Header(ElevationOperation.Delete));

        // Join first, and deliberately: a helper that accepted the second header would sit waiting
        // for items that are never coming, and reading before joining would wait with it forever
        // rather than failing.
        Assert.True(host.Join(Patience), "a second header should end the session.");
        Assert.Equal(ElevationVerb.Fatal, app.Receive()!.Value.Verb);
    }

    [Fact]
    public void AnItemBeforeAHeaderIsRefused()
    {
        var (host, app) = Hosted();
        Drain(app, ElevationVerb.Ready);

        app.Send(ElevationVerb.Item, RenameItem("a.txt", "b.txt"));

        Assert.Equal(ElevationVerb.Fatal, app.Receive()!.Value.Verb);
        Assert.True(host.Join(Patience));
    }

    [Fact]
    public void MoreItemsThanTheCapRefusesTheWholeRequest()
    {
        var (host, app) = Hosted();
        Drain(app, ElevationVerb.Ready);
        var victim = File_("here", "a.txt");

        app.Send(ElevationVerb.Hello, ElevationProtocol.ProtocolVersion.ToString());
        app.Send(ElevationVerb.Begin, Header(ElevationOperation.Rename));
        for (var i = 0; i <= ElevationProtocol.MaxItems; i++)
            app.Send(ElevationVerb.Item, RenameItem("a.txt", "b.txt"));

        Assert.Equal(ElevationVerb.Fatal, app.Receive()!.Value.Verb);
        Assert.True(host.Join(Patience));
        Assert.True(File.Exists(victim));
    }

    // --- what it refuses to touch ---

    [Theory]
    [InlineData(@"..\outside.txt")]
    [InlineData(@"\\.\PhysicalDrive0")]
    [InlineData(@"\\?\C:\Windows\evil.txt")]
    [InlineData(@"C:\a*.txt")]
    public void APathTheIpcRuleRefusesIsRefusedWhole(string source)
    {
        var (host, app) = Hosted();
        Drain(app, ElevationVerb.Ready);
        var bystander = File_("untouched", "a.txt");

        app.Request(
            Header(ElevationOperation.Rename),
            ElevationHost.Write(new PlannedRename(source, P("b.txt"), false)),
            RenameItem("a.txt", "c.txt"));

        Assert.Contains(app.ReceiveAll(), m => m.Verb == ElevationVerb.Fatal);
        Assert.True(host.Join(Patience));

        // Refused whole, not in part: the good item in the same request was not carried out either.
        Assert.True(File.Exists(bystander));
        Assert.False(File.Exists(P("c.txt")));
    }

    [Fact]
    public void AnUnreadableItemRefusesTheWholeRequest()
    {
        var (host, app) = Hosted();
        Drain(app, ElevationVerb.Ready);
        var bystander = File_("untouched", "a.txt");

        app.Request(Header(ElevationOperation.Rename), "{not json at all", RenameItem("a.txt", "c.txt"));

        Assert.Contains(app.ReceiveAll(), m => m.Verb == ElevationVerb.Fatal);
        Assert.True(host.Join(Patience));
        Assert.True(File.Exists(bystander));
        Assert.False(File.Exists(P("c.txt")));
    }

    // --- doing the work ---

    [Fact]
    public void ItRenamesAndReportsWhatItDid()
    {
        var (host, app) = Hosted();
        Drain(app, ElevationVerb.Ready);
        File_("payload", "a.txt");

        app.Request(Header(ElevationOperation.Rename), RenameItem("a.txt", "b.txt"));
        var said = app.ReceiveAll();
        host.Join(Patience);

        Assert.Equal("payload", File.ReadAllText(P("b.txt")));
        Assert.Contains(said, m => m.Verb == ElevationVerb.Done);
        Assert.Contains(said, m => m.Verb == ElevationVerb.End);
        Assert.DoesNotContain(said, m => m.Verb == ElevationVerb.Fault);
    }

    [Fact]
    public void ItMovesAndReportsProgressAsItGoes()
    {
        var (host, app) = Hosted();
        Drain(app, ElevationVerb.Ready);
        var source = File_("payload", "src", "a.txt");
        var dest = Dir("dest");

        app.Request(
            ElevationHost.Write(new ElevationHeader(ElevationOperation.TransferMove, dest)),
            ElevationHost.Write(new ElevationTransferItem(
                new PlannedTransfer(source, false, Path.Combine(dest, "a.txt"), false),
                ConflictResolution.KeepBoth)));

        var said = app.ReceiveAll();
        host.Join(Patience);

        Assert.Equal("payload", File.ReadAllText(Path.Combine(dest, "a.txt")));
        Assert.False(File.Exists(source));
        Assert.Contains(said, m => m.Verb == ElevationVerb.Progress);
        Assert.Contains(said, m => m.Verb == ElevationVerb.End);
    }

    [Fact]
    public void ItCreatesTheOneItemACreationNames()
    {
        var (host, app) = Hosted();
        Drain(app, ElevationVerb.Ready);

        app.Request(
            Header(ElevationOperation.NewItem),
            ElevationHost.Write(new NewItemPlan(_root, "notes.txt", NewItemKind.File, null, null)));

        var said = app.ReceiveAll();
        host.Join(Patience);

        Assert.True(File.Exists(P("notes.txt")));
        Assert.Contains(said, m => m.Verb == ElevationVerb.Done);
    }

    [Fact]
    public void AFailureIsReportedWithoutStoppingTheRest()
    {
        var (host, app) = Hosted();
        Drain(app, ElevationVerb.Ready);
        File_("here", "a.txt");

        app.Request(
            Header(ElevationOperation.Rename),
            RenameItem("missing.txt", "gone.txt"),
            RenameItem("a.txt", "b.txt"));

        var said = app.ReceiveAll();
        host.Join(Patience);

        Assert.Contains(said, m => m.Verb == ElevationVerb.Fault);
        Assert.True(File.Exists(P("b.txt")));
    }

    // --- the app going away ---

    [Fact]
    public void LosingTheAppEndsTheRun()
    {
        var (host, app) = Hosted();
        Drain(app, ElevationVerb.Ready);

        app.Close();

        Assert.True(host.Join(Patience), "the helper should end when the pipe does.");
    }

    // --- helpers ---

    private (Thread Thread, ElevationPeer App) Hosted()
    {
        var (helperSide, appSide) = DuplexPair.Create();
        var thread = new Thread(() => new ElevationHost(helperSide).Run()) { IsBackground = true };
        thread.Start();
        return (thread, new ElevationPeer(appSide));
    }

    private static void Drain(ElevationPeer app, ElevationVerb until)
    {
        while (app.Receive() is { } message && message.Verb != until)
        {
        }
    }

    private static string Header(ElevationOperation operation) =>
        ElevationHost.Write(new ElevationHeader(operation));

    private string RenameItem(string from, string to) =>
        ElevationHost.Write(new PlannedRename(P(from), P(to), false));

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
}
