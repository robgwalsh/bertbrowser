using System.IO.Pipes;
using System.Security.Principal;
using BertBrowser.Core.Data;
using BertBrowser.Core.Ipc;
using BertBrowser.Core.Services.Mft;

namespace BertBrowser.Indexer;

/// <summary>
/// BertBrowser's index helper: the one process in the product that runs with an administrator
/// token, because reading the NTFS MFT needs a raw volume handle and nothing else does.
/// </summary>
/// <remarks>
/// <para>
/// It is started by the app, connects back to a pipe the app is listening on, and takes four verbs
/// — hello, start, shutdown, ping. It never receives a path, never launches anything, and never
/// draws a window. Everything it produces goes into the database the app already created.
/// </para>
/// <para>
/// It exits when the app does. See <see cref="PipeOwner"/> for the two mechanisms and why
/// neither is the explicit shutdown message.
/// </para>
/// </remarks>
internal static class Program
{
    /// <summary>Generous: the app has just been through a UAC prompt and may still be laying out.</summary>
    private static readonly TimeSpan ConnectTimeout = TimeSpan.FromSeconds(30);

    private static int Main(string[] args)
    {
        if (!IndexerArguments.TryParse(args, out var options, out var error))
        {
            Console.Error.WriteLine(error);
            return 2;
        }

        try
        {
            return Run(options);
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine(ex.Message);
            return 1;
        }
    }

    private static int Run(IndexerArguments options)
    {
        // PipeOptions.Asynchronous, and this is load-bearing rather than a performance choice.
        // Windows serializes I/O on a non-overlapped handle: with PipeOptions.None, this process's
        // main thread parked in a blocking read would block the volume threads' writes on the same
        // handle, so not one progress message could leave while the app sat waiting for exactly
        // those messages. Overlapped I/O lets the two directions proceed independently. The reads
        // and writes below stay synchronous; only the handle changes.
        using var pipe = new NamedPipeClientStream(
            ".", options.PipeName, PipeDirection.InOut, PipeOptions.Asynchronous,
            TokenImpersonationLevel.Identification);

        try
        {
            pipe.Connect((int)ConnectTimeout.TotalMilliseconds);
        }
        catch (Exception ex) when (ex is TimeoutException or IOException)
        {
            Console.Error.WriteLine("The app is not listening.");
            return 3;
        }

        // The name could have been guessed or raced for. Only this establishes that the endpoint
        // belongs to the process that started us.
        if (!PipeOwner.OwnsPipe(pipe, options.ParentProcessId))
        {
            Console.Error.WriteLine("The pipe does not belong to the process that started this one.");
            return 4;
        }

        var dbPath = Path.Combine(options.DataDirectory, "bertbrowser.db");
        if (!File.Exists(dbPath))
        {
            // The app creates and migrates the database under its own token, before it ever starts
            // this process. Creating one here would leave an Administrators-owned file behind.
            Console.Error.WriteLine("No database at the given data directory.");
            return 5;
        }

        // Never Migrate(): the schema belongs to the app, which has already applied it.
        var db = new Db(dbPath, create: false);
        using var index = new MftIndexService(new FsIndexRepository(db), new DirSizeRepository(db));

        using var lifetime = new CancellationTokenSource();
        PipeOwner.WatchForExit(options.ParentProcessId, () =>
        {
            // Belt to the pipe's braces. Cancelling ends Run, which returns from Main, which closes
            // the volume handles the indexer holds.
            try { lifetime.Cancel(); } catch (ObjectDisposedException) { }
            try { pipe.Dispose(); } catch (Exception ex) when (ex is IOException or ObjectDisposedException) { }
        });

        new MftIndexHost(index, pipe).Run(lifetime.Token);
        return 0;
    }
}
