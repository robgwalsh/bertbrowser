using System.Security;

namespace BertBrowser.Core.Services.Mft;

/// <summary>
/// The Windows scheduled task that starts the index helper at sign-in, so it is already running —
/// and already elevated — before the app is ever opened.
/// </summary>
/// <remarks>
/// <para>
/// This is the only way to never see the elevation prompt again: a task registered with
/// <c>HighestAvailable</c> runs elevated without one. Registering it needs elevation once, which is
/// exactly the gesture that should stand in front of "install something that runs with an
/// administrator token at every sign-in" — so the app does it by running the helper through the
/// same <c>runas</c> path it uses to start it, rather than by adding a verb to the pipe. A verb
/// would let anything reaching the pipe install that persistence with no user gesture at all.
/// </para>
/// <para>
/// Pure, and the tests are the specification. Every element below is load-bearing and at least one
/// of them is a silent default that would otherwise break the feature outright — see
/// <see cref="BuildXml"/>.
/// </para>
/// </remarks>
public static class IndexerAutoStartTask
{
    /// <summary>What the task is called, in the task scheduler's root folder.</summary>
    public const string TaskName = "BertBrowser Index Helper";

    /// <summary>
    /// The task definition, as Task Scheduler 1.2 XML.
    /// </summary>
    /// <param name="helperPath">Full path to <c>BertBrowser.Indexer.exe</c>.</param>
    /// <param name="dataDirectory">
    /// Where the database is. <b>This must be the user's real profile directory, never an
    /// overridden one</b> — a harness or test run that registered a task would otherwise leave an
    /// elevated process being started at every sign-in, pointed at a scratch directory that no
    /// longer exists.
    /// </param>
    /// <param name="userSid">
    /// Who the task runs as. It must be the current user rather than SYSTEM: the helper derives
    /// both its pipe name and its data directory from its own token, so a SYSTEM helper would
    /// listen on a name no app looks for and write Administrators-owned rows into the wrong place.
    /// </param>
    public static string BuildXml(string helperPath, string dataDirectory, string userSid)
    {
        var arguments = $"--data-dir \"{dataDirectory.TrimEnd('\\')}\"";

        return $"""
        <?xml version="1.0" encoding="UTF-16"?>
        <Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
          <RegistrationInfo>
            <Description>Keeps BertBrowser's search index up to date. Started at sign-in so BertBrowser never has to ask for administrator rights.</Description>
            <URI>\{Escape(TaskName)}</URI>
          </RegistrationInfo>
          <Triggers>
            <LogonTrigger>
              <Enabled>true</Enabled>
              <UserId>{Escape(userSid)}</UserId>
            </LogonTrigger>
          </Triggers>
          <Principals>
            <Principal id="Author">
              <UserId>{Escape(userSid)}</UserId>
              <LogonType>InteractiveToken</LogonType>
              <RunLevel>HighestAvailable</RunLevel>
            </Principal>
          </Principals>
          <!-- The element order here is fixed by the Task Scheduler schema, not a matter of taste:
               a misplaced one is rejected outright with a line and column and nothing else. -->
          <Settings>
            <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
            <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
            <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
            <AllowHardTerminate>true</AllowHardTerminate>
            <StartWhenAvailable>false</StartWhenAvailable>
            <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
            <IdleSettings>
              <StopOnIdleEnd>false</StopOnIdleEnd>
              <RestartOnIdle>false</RestartOnIdle>
            </IdleSettings>
            <AllowStartOnDemand>true</AllowStartOnDemand>
            <Enabled>true</Enabled>
            <Hidden>false</Hidden>
            <RunOnlyIfIdle>false</RunOnlyIfIdle>
            <!-- DisallowStartOnRemoteAppSession and UseUnifiedSchedulingEngine are deliberately
                 absent: they need schema 1.3, this declares 1.2 for the widest compatibility, and
                 neither buys anything here. -->
            <WakeToRun>false</WakeToRun>
            <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
            <Priority>7</Priority>
          </Settings>
          <Actions Context="Author">
            <Exec>
              <Command>{Escape(helperPath)}</Command>
              <Arguments>{Escape(arguments)}</Arguments>
            </Exec>
          </Actions>
        </Task>
        """;
    }

    /// <summary>
    /// True when a registered task points somewhere other than where the helper now lives.
    /// </summary>
    /// <remarks>
    /// Velopack replaces the install directory on an update, which leaves the task aimed at an
    /// executable that is gone — the sign-in start would then silently do nothing, and the user
    /// would only notice as a prompt they thought they had abolished. It cannot be repaired without
    /// a prompt, so the settings page says so rather than pretending.
    /// </remarks>
    public static bool NeedsReregistration(string? registeredCommand, string currentHelperPath)
    {
        if (string.IsNullOrWhiteSpace(registeredCommand)) return false;

        return !Paths.PathKey.Canonicalize(registeredCommand.Trim('"'))
            .Equals(Paths.PathKey.Canonicalize(currentHelperPath), StringComparison.Ordinal);
    }

    /// <summary>
    /// XML-escapes a value. A profile path may perfectly well contain an ampersand, and an
    /// unescaped one makes the whole definition unparseable.
    /// </summary>
    private static string Escape(string value) =>
        SecurityElement.Escape(value) ?? string.Empty;
}
