using System.IO;
using System.Xml.Linq;
using BertBrowser.App.Services.Elevation;
using BertBrowser.Core.Services.Mft;

namespace BertBrowser.App.Services.Indexing;

/// <summary>Whether the index helper starts at sign-in, and whether that can be established.</summary>
public enum AutoStartState
{
    /// <summary>No sign-in task is registered.</summary>
    Off,

    /// <summary>Registered, and pointing at the helper beside this build.</summary>
    On,

    /// <summary>
    /// Registered, but pointing at an executable that is no longer there — an update moved the
    /// install directory out from under it.
    /// </summary>
    Stale,

    /// <summary>
    /// It could not be read. <b>Rendered as off</b>, never as on: a ticked box nobody verified is a
    /// promise the app has not checked it can keep.
    /// </summary>
    Unknown,
}

/// <summary>
/// Turns the sign-in task on and off, by running the elevated helper with an argument.
/// </summary>
/// <remarks>
/// <para>
/// One elevation prompt per toggle, and that prompt is the whole point — it is the user's gesture
/// in front of installing something that runs with an administrator token at every sign-in. The
/// alternative, a verb on the index pipe, would have let anything that reaches that pipe install
/// the same persistence silently. See <c>IndexProtocol</c>.
/// </para>
/// <para>
/// <b>The task is the state.</b> Nothing about this is kept in settings, exactly as the folder
/// handler keeps its state in the registry rather than beside it: a stored flag and a real
/// registration drift apart the moment anyone touches the task from outside, and then the checkbox
/// is lying.
/// </para>
/// </remarks>
public sealed class IndexAutoStartService
{
    private readonly string _helperPath;

    public IndexAutoStartService()
        : this(Path.Combine(AppContext.BaseDirectory, "BertBrowser.Indexer.exe"))
    {
    }

    internal IndexAutoStartService(string helperPath) => _helperPath = helperPath;

    /// <summary>Whether asking is even possible on this account.</summary>
    public bool CanElevate => ElevatedProcess.CanElevate;

    /// <summary>
    /// Registers or removes the sign-in task, returning false when it did not happen — a declined
    /// prompt included.
    /// </summary>
    public bool TrySet(bool on)
    {
        var verb = on ? "--register-autostart" : "--unregister-autostart";
        var arguments = $"{verb} --data-dir \"{DataDirectoryForTask().TrimEnd('\\')}\"";

        var started = ElevatedProcess.Start(_helperPath, arguments, "the index helper is missing");
        if (started.Outcome != ElevatedStart.Started) return false;

        ElevatedProcess.WaitForExit(started.Handle, TimeSpan.FromSeconds(30));

        // Verified against the scheduler rather than taken on trust. The helper exits without a
        // channel to report on, and a checkbox that ticked itself because a process started is
        // exactly the lie this design is trying not to tell.
        var state = State();
        return on
            ? state is AutoStartState.On or AutoStartState.Stale
            : state == AutoStartState.Off;
    }

    /// <summary>
    /// What the scheduler actually holds.
    /// </summary>
    /// <remarks>
    /// Read from the task's own XML under <c>%WINDIR%\System32\Tasks</c>. A medium-integrity process
    /// can usually read it; when it cannot, the answer is <see cref="AutoStartState.Unknown"/>
    /// rather than a guess in either direction.
    /// </remarks>
    public AutoStartState State()
    {
        try
        {
            var path = Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.Windows),
                "System32", "Tasks", IndexerAutoStartTask.TaskName);

            if (!File.Exists(path)) return AutoStartState.Off;

            var command = XDocument.Load(path)
                .Descendants()
                .FirstOrDefault(e => e.Name.LocalName == "Command")?.Value;

            return IndexerAutoStartTask.NeedsReregistration(command, _helperPath)
                ? AutoStartState.Stale
                : AutoStartState.On;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                                      or System.Xml.XmlException or ArgumentException)
        {
            return AutoStartState.Unknown;
        }
    }

    /// <summary>
    /// The directory the task should point at — always the real profile one.
    /// </summary>
    /// <remarks>
    /// <b>Never <c>AppPaths.DataDir</c>.</b> That honours <c>BERTBROWSER_DATA_DIR</c>, which the UI
    /// harness sets to a scratch directory it deletes afterwards. A run that registered a task would
    /// otherwise leave an elevated process being started at every sign-in, aimed at a folder that no
    /// longer exists.
    /// </remarks>
    private static string DataDirectoryForTask() =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".bertbrowser");
}
