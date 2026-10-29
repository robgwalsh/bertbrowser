using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using BertBrowser.Core.Ipc;
using BertBrowser.Core.Services.Mft;

namespace BertBrowser.Indexer;

/// <summary>
/// Registers and removes the sign-in task, from inside the already-elevated helper.
/// </summary>
/// <remarks>
/// <para>
/// Reached only through <c>--register-autostart</c> / <c>--unregister-autostart</c>, which the app
/// runs through the same <c>runas</c> path it uses to start the helper. One elevation prompt per
/// toggle, and that prompt is the point: it is the user's gesture in front of installing something
/// that runs with an administrator token at every sign-in. Adding a verb to the pipe instead would
/// have let anything reaching that pipe install the same persistence with no gesture at all.
/// </para>
/// <para>
/// <b>The Task Scheduler COM API rather than <c>schtasks.exe</c>.</b> <c>schtasks /Create /XML</c>
/// needs the definition written to a file first — from an elevated process, so an
/// Administrators-owned file, with a window between writing it and the tool reading it.
/// <c>ITaskFolder.RegisterTask</c> takes the XML as a string: no file, no window, no child process.
/// </para>
/// <para>
/// <b>Late-bound through <c>IDispatch</c>, deliberately.</b> These are dual interfaces, so
/// <c>dynamic</c> reaches them the same way every scripting host does. Hand-declaring the vtables
/// would mean writing out every method slot before the two that are actually called, in the right
/// order, with the right signatures — and getting one wrong is not a compile error or an exception
/// but a call through the wrong function pointer.
/// </para>
/// </remarks>
[SupportedOSPlatform("windows")]
internal static class AutoStart
{
    /// <summary>TASK_CREATE_OR_UPDATE.</summary>
    private const int CreateOrUpdate = 6;

    /// <summary>TASK_LOGON_INTERACTIVE_TOKEN — run as the signed-in user, with their own token.</summary>
    private const int LogonInteractiveToken = 3;

    private const int ErrorFileNotFound = unchecked((int)0x80070002);

    public static int Register(string dataDirectory)
    {
        var helperPath = Environment.ProcessPath;
        if (helperPath is null)
        {
            Console.Error.WriteLine("Cannot determine this executable's path.");
            return 8;
        }

        var xml = IndexerAutoStartTask.BuildXml(
            helperPath, dataDirectory, IndexEndpoint.CurrentUserSid());

        return WithRootFolder(folder => folder.RegisterTask(
            IndexerAutoStartTask.TaskName, xml, CreateOrUpdate,
            Type.Missing, Type.Missing, LogonInteractiveToken, Type.Missing));
    }

    public static int Unregister() => WithRootFolder(folder =>
    {
        try
        {
            folder.DeleteTask(IndexerAutoStartTask.TaskName, 0);
        }
        catch (COMException ex) when (ex.HResult == ErrorFileNotFound)
        {
            // Already gone is the outcome that was asked for.
        }
        catch (FileNotFoundException)
        {
        }
    });

    private static int WithRootFolder(Action<dynamic> work)
    {
        var type = Type.GetTypeFromProgID("Schedule.Service");
        if (type is null)
        {
            Console.Error.WriteLine("The Task Scheduler service is not available.");
            return 9;
        }

        object? service = null;
        try
        {
            service = Activator.CreateInstance(type);
            if (service is null)
            {
                Console.Error.WriteLine("The Task Scheduler service could not be created.");
                return 9;
            }

            dynamic scheduler = service;
            scheduler.Connect();
            work(scheduler.GetFolder("\\"));
            return 0;
        }
        catch (Exception ex) when (ex is COMException or UnauthorizedAccessException
                                      or InvalidOperationException or MissingMemberException)
        {
            Console.Error.WriteLine($"The sign-in task could not be changed: {ex.Message}");
            return 10;
        }
        finally
        {
            if (service is not null) Marshal.FinalReleaseComObject(service);
        }
    }
}
