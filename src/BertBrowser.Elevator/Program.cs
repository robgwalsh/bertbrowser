using System.IO.Pipes;
using System.Security.Principal;
using BertBrowser.Core.Ipc;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Elevation;

namespace BertBrowser.Elevator;

/// <summary>
/// BertBrowser's file-operation helper: the process that carries out one move, copy, delete, rename
/// or creation the user's own token was refused, and then exits.
/// </summary>
/// <remarks>
/// <para>
/// It is started by the app — only ever from a click on a shield in a dialog naming the items —
/// connects back to a pipe the app is already listening on, takes exactly one request, does it
/// through the same Core executors the app uses, reports what happened, and returns. It never opens
/// a window, never launches a program, and never opens the database.
/// </para>
/// <para>
/// The index helper's rule is "four verbs and never a path", and it survives unchanged: that rule is
/// about a helper which lives for the whole session, is started at launch without anyone asking, and
/// whose job names no file. This one inverts all three, and what replaces the rule is one prompt per
/// operation, one request per process, and a process that exits when the request is done.
/// </para>
/// <para>
/// It exits when the app does. See <see cref="PipeOwner"/> for the two mechanisms and why neither is
/// the explicit shutdown message.
/// </para>
/// </remarks>
// Windows-only APIs on a plain net10.0 project, so the platform-compatibility analyzer wants the
// promise spelled out. The whole product is Windows-only; this is bookkeeping, not a constraint.
[System.Runtime.Versioning.SupportedOSPlatform("windows")]
internal static class Program
{
    /// <summary>Generous: the app has just been through a UAC prompt.</summary>
    private static readonly TimeSpan ConnectTimeout = TimeSpan.FromSeconds(30);

    private static int Main(string[] args)
    {
        if (!ElevatorArguments.TryParse(args, out var options, out var error))
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

    private static int Run(ElevatorArguments options)
    {
        // PipeOptions.Asynchronous, and this is load-bearing rather than a performance choice: with
        // PipeOptions.None, Windows serialises I/O on the handle, so this process's read loop waiting
        // for a Cancel would block the worker thread's progress writes on the same handle — see the
        // index helper's own note, which was written after exactly that deadlock.
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

        // A coherence check rather than a boundary — see PipeOwner.ImagePathOf. It rules out a
        // stale build or a raced name; what protects the user is the prompt they answered.
        if (!IsOurApp(options.ParentProcessId))
        {
            Console.Error.WriteLine("The process that started this one is not the app beside it.");
            return 5;
        }

        // UAC gives the same user a different token, not a different user, so this identity is the
        // caller's. The argument exists only so a mismatch can be refused, which covers the
        // over-the-shoulder credential prompt where the elevating account is somebody else's.
        var self = WindowsIdentity.GetCurrent().User?.Value;
        if (self is null || !string.Equals(self, options.UserSid, StringComparison.OrdinalIgnoreCase))
        {
            Console.Error.WriteLine("This helper is not running as the user that started it.");
            return 6;
        }

        using var lifetime = new CancellationTokenSource();
        PipeOwner.WatchForExit(options.ParentProcessId, () =>
        {
            // Belt to the pipe's braces. Cancelling ends the operation the way the Cancel verb does,
            // and the executors' own guarantees mean nothing is left half-written.
            try { lifetime.Cancel(); } catch (ObjectDisposedException) { }
            try { pipe.Dispose(); } catch (Exception ex) when (ex is IOException or ObjectDisposedException) { }
        });

        // One object serving both roles, as the app registers it: the planner and the executor must
        // agree about what has a Recycle Bin.
        var bin = new ShellRecycleBin();
        new ElevationHost(pipe, bin, bin, StagingAcl.GrantTo(self)).Run(lifetime.Token);
        return 0;
    }

    /// <summary>Whether the process on the other end is the BertBrowser sitting beside this helper.
    /// Compared through <see cref="PathKey"/> and never ordinally, or a casing difference would read
    /// as a mismatch.</summary>
    private static bool IsOurApp(int processId)
    {
        if (PipeOwner.ImagePathOf(processId) is not { } image) return false;

        var expected = Path.Combine(AppContext.BaseDirectory, "BertBrowser.exe");
        try
        {
            return string.Equals(
                PathKey.Canonicalize(image), PathKey.Canonicalize(expected), StringComparison.Ordinal);
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
        {
            return false;
        }
    }
}
