using System.IO.Pipes;
using System.Security.Principal;
using BertBrowser.Core.Data;
using BertBrowser.Core.Ipc;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Changes;
using BertBrowser.Core.Services.Mft;

namespace BertBrowser.Indexer;

/// <summary>
/// BertBrowser's index helper: the one process in the product that runs with an administrator
/// token, because reading the NTFS MFT needs a raw volume handle and nothing else does.
/// </summary>
/// <remarks>
/// <para>
/// It takes five verbs — hello, start, shutdown, ping, and record (one integer: how long to keep a
/// change log, 0 for not at all). It never receives a path, never launches anything, and never
/// draws a window. Everything it produces goes into the database the app already created.
/// </para>
/// <para>
/// <b>It outlives the app.</b> That is the whole point of it: an app that closes and reopens
/// attaches to the index and the journal tail already running here, so it costs no elevation prompt
/// and no rebuild. A lost pipe ends a <em>session</em>; this process waits for the next app.
/// </para>
/// <para>
/// So the three things that end it are worth stating plainly, since nothing supervises it any more:
/// an app asking (<see cref="IndexVerb.Shutdown"/>), the user signing out, and
/// <see cref="SelfChecks"/> — which stop it when the app it belongs to has been updated, removed,
/// or has migrated the database out from under it.
/// </para>
/// <para>
/// It is also single-instance, and that is not a nicety. Two of these would tail one volume's
/// journal into one SQLite file while fighting over one pipe name, and there are now two
/// independent ways to start one: a click in the app, and the sign-in task.
/// </para>
/// </remarks>
[System.Runtime.Versioning.SupportedOSPlatform("windows")]
internal static class Program
{
    /// <summary>How often to look for a reason to stop. See <see cref="SelfChecks"/>.</summary>
    private static readonly TimeSpan SelfCheckInterval = TimeSpan.FromSeconds(30);

    /// <summary>
    /// How long to wait for an app between connection attempts.
    /// </summary>
    /// <remarks>
    /// An explicit cadence rather than one long blocking connect, so how quickly a starting app is
    /// picked up is a number chosen here rather than whatever the framework's retry loop happens to
    /// do. A quarter of a second is imperceptible against a window opening.
    /// </remarks>
    private static readonly TimeSpan ConnectPollInterval = TimeSpan.FromMilliseconds(250);

    private static int Main(string[] args)
    {
        if (!IndexerArguments.TryParse(args, out var options, out var error))
        {
            Console.Error.WriteLine(error);
            return 2;
        }

        try
        {
            return options.Command switch
            {
                IndexerCommand.RegisterAutoStart => AutoStart.Register(options.DataDirectory),
                IndexerCommand.UnregisterAutoStart => AutoStart.Unregister(),
                _ => Run(options),
            };
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine(ex.Message);
            return 1;
        }
    }

    private static int Run(IndexerArguments options)
    {
        var sid = IndexEndpoint.CurrentUserSid();

        // One helper per user, or two would index the same volumes into the same database.
        using var claim = IndexerPresenceLock.TryAcquire(sid);
        if (claim is null)
        {
            Console.Error.WriteLine("Another index helper is already running.");
            return 6;
        }

        var dbPath = Path.Combine(options.DataDirectory, "bertbrowser.db");
        if (!File.Exists(dbPath))
        {
            // The app creates and migrates the database under its own token, before it ever starts
            // this process. Creating one here would leave an Administrators-owned file behind — and
            // at sign-in, before the app has ever run, exiting quietly is the right answer.
            Console.Error.WriteLine("No database at the given data directory.");
            return 5;
        }

        // Never Migrate(): the schema belongs to the app, which has already applied it.
        var db = new Db(dbPath, create: false);
        if (db.SchemaVersion != Db.ExpectedSchemaVersion)
        {
            // An app of a different build owns this database. Writing rows against a schema this
            // build does not know is worse than not indexing.
            Console.Error.WriteLine("The database is at a different schema version than this helper.");
            return 7;
        }

        // The change log's writer lives here too, because the records come from the same tail.
        // Off until the app says otherwise (IndexVerb.Record), and it never records its own
        // database's growth — the data directory is what the exclusion is. It keeps whatever the
        // last app pushed while none is connected, so a session closing leaves no gap in the log.
        var changes = new ChangeRecorderOptions(
            new ChangeLogRepository(db), PathKey.Canonicalize(options.DataDirectory));
        using var index = new MftIndexService(new FsIndexRepository(db), new DirSizeRepository(db), changes);

        using var lifetime = new CancellationTokenSource();
        using var selfChecks = SelfChecks.Start(db, lifetime);

        // Indexing is this process's job whether or not anyone is watching: started by the sign-in
        // task there may be no app for hours, and the whole point is that the index is warm when
        // one arrives.
        index.Start();

        var host = new MftIndexHost(index);
        return Serve(host, sid, lifetime.Token);
    }

    /// <summary>
    /// Hosts one app after another until asked to stop.
    /// </summary>
    private static int Serve(MftIndexHost host, string sid, CancellationToken ct)
    {
        var endpoint = IndexEndpoint.ForUser(sid);

        while (!ct.IsCancellationRequested)
        {
            // A fresh stream per attempt: a NamedPipeClientStream cannot be reconnected.
            // PipeOptions.Asynchronous, and this is load-bearing rather than a performance choice.
            // Windows serializes I/O on a non-overlapped handle: with PipeOptions.None, this
            // process's main thread parked in a blocking read would block the volume threads'
            // writes on the same handle, so not one progress message could leave while the app sat
            // waiting for exactly those messages. Overlapped I/O lets the two directions proceed
            // independently. The reads and writes below stay synchronous; only the handle changes.
            var pipe = new NamedPipeClientStream(
                ".", endpoint, PipeDirection.InOut, PipeOptions.Asynchronous,
                TokenImpersonationLevel.Identification);

            try
            {
                try
                {
                    pipe.Connect((int)ConnectPollInterval.TotalMilliseconds);
                }
                catch (Exception ex) when (ex is TimeoutException or IOException)
                {
                    // No app is listening yet. Indexing carries on regardless.
                    continue;
                }

                if (!IsOurApp(pipe))
                {
                    // Not the app this helper ships beside. Hang up rather than report to it.
                    continue;
                }

                if (host.Run(pipe, ct) == IndexSessionEnd.Stop) return 0;
            }
            catch (Exception ex) when (ex is IOException or ObjectDisposedException)
            {
                // A broken session is the ordinary way an app goes away.
            }
            finally
            {
                pipe.Dispose();
            }
        }

        return 0;
    }

    /// <summary>
    /// Whether the process listening on the pipe is the BertBrowser this helper ships beside.
    /// </summary>
    /// <remarks>
    /// <b>A coherence check, not a boundary.</b> Nothing between two processes of one user is one —
    /// a program running as this user could raise its own prompt or copy the command line. What
    /// this rules out is confusion: a stale build, or another program that happened to claim the
    /// name. It replaces the parent-process check the helper used to make, which stopped meaning
    /// anything once it outlived whoever launched it. The direction works because a high-integrity
    /// process may open a medium-integrity one for <c>PROCESS_QUERY_LIMITED_INFORMATION</c>; the
    /// reverse would not.
    /// </remarks>
    private static bool IsOurApp(NamedPipeClientStream pipe)
    {
        if (!PipeOwner.TryGetServerProcessId(pipe, out var serverProcessId)) return false;

        var actual = PipeOwner.ImagePathOf(serverProcessId);
        if (actual is null) return false;

        var expected = Path.Combine(AppContext.BaseDirectory, "BertBrowser.exe");
        return PathKey.Canonicalize(actual).Equals(PathKey.Canonicalize(expected), StringComparison.Ordinal);
    }
}
