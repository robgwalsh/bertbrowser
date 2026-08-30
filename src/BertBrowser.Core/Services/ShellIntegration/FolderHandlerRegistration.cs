namespace BertBrowser.Core.Services.ShellIntegration;

/// <summary>One value the registration writes. <see cref="ValueName"/> is <c>""</c> for a key's
/// default value, which is what the shell reads for both a verb name and a command line.</summary>
public sealed record ShellRegistryValue(string KeyPath, string ValueName, string Data);

/// <summary>
/// One value the registration removes, and the data it must still hold to be removable.
/// </summary>
/// <remarks>
/// The expected data is the safety catch. These values sit on keys shared with other programs, so
/// removal deletes only what this app actually wrote — a default verb someone else has since
/// pointed elsewhere is theirs, and an unregister that blanked it would break their registration
/// while claiming to have tidied up after ours.
/// </remarks>
public sealed record ShellRegistryValueRef(string KeyPath, string ValueName, string ExpectedData);

/// <summary>
/// What it takes to make the Windows shell open a folder or a drive in BertBrowser instead of
/// Explorer, described as data so the writer, the remover, the status reader and the tests cannot
/// disagree about it.
/// </summary>
/// <remarks>
/// <para>
/// <b>Windows has no "default file manager" setting</b>, and this is not an oversight to work
/// around with a better API. File Explorer's own <c>Capabilities</c> key registers three URL
/// associations (burn, erase, zip) and no file or folder associations at all, so nothing in
/// Default Apps, <c>RegisteredApplications</c> or <c>IApplicationAssociationRegistration</c> can
/// express "open folders in this program". Overriding the shell verb is the only mechanism there
/// is.
/// </para>
/// <para>
/// <b>The scope is <c>Directory</c> and <c>Drive</c>, deliberately never <c>Folder</c>.</b>
/// <c>HKLM\Software\Classes\Directory\shell\open</c> and <c>Drive\shell\open</c> do not exist:
/// folders and drives inherit their open verb from the <c>Folder</c> class, whose command is
/// <c>Explorer.exe</c> behind a <c>DelegateExecute</c> CLSID. Writing <i>more specific</i> verbs on
/// <c>Directory</c> and <c>Drive</c> therefore takes the filesystem cases and leaves <c>Folder</c>
/// alone — This PC, Control Panel, zip browsing, FTP and Explorer's own internal navigation all
/// keep working, and keep working even if this registration is left pointing at nothing. Taking
/// <c>Folder</c> instead would route namespace folders BertBrowser cannot browse at it, and would
/// need a hand-back to Explorer for each one. That trade is not worth making.
/// </para>
/// <para>
/// <b>The <c>(Default)</c> on the <c>shell</c> key has to be written, and writing it is the most
/// dangerous thing here.</b> Both <c>Directory\shell</c> and <c>Drive\shell</c> ship with
/// <c>"none"</c>. Measured on a real machine: with <c>"none"</c> in place the shell uses its own
/// built-in folder navigation and never consults a verb at all — a complete, correct
/// <c>Directory\shell\open</c> registration is simply ignored, `DelegateExecute` blanked or not.
/// Naming a verb there is what makes the shell invoke one.
/// </para>
/// <para>
/// The danger is what happens when the named verb is <i>missing</i>: the shell falls through to the
/// first verb it enumerates, which is whatever a third party happens to have installed. An early
/// version of this file wrote this value <b>first</b>, before creating the verb — and on a machine
/// with a NordVPN entry under <c>HKCU\...\Directory\shell</c>, <b>double-clicking a folder opened
/// NordVPN</b>. Hence <see cref="GuardValues"/> is separate from <see cref="ValuesFor"/> and is
/// written last, only after the command has been read back from the registry, and rolled back
/// together with everything else if any part of the write fails. Ordering is not a detail here; it
/// is the difference between a feature and a hijacked machine.
/// </para>
/// <para>
/// <b><c>DelegateExecute = ""</c> is mandatory, and reasoning about HKCU/HKLM instead of class
/// inheritance is how that gets missed.</b> There is no HKLM <c>Directory\shell\open</c> key to
/// mask, which makes it look unnecessary — but <c>Directory</c> and <c>Drive</c> <i>derive from</i>
/// <c>Folder</c>, and the verb they inherit carries <c>CLSID_ExecuteFolder</c>. A resolving
/// <c>DelegateExecute</c> beats the command line outright, so without the empty shadow every folder
/// went to Explorer while the keys looked exactly right. See <see cref="DelegateExecute"/>.
/// </para>
/// <para>
/// <b><c>%1</c> on a drive root expands to <c>C:\</c></b>, whose trailing backslash escapes the
/// closing quote and delivers <c>C:\"</c> to <c>argv</c>. That is already handled:
/// <c>CommandLine.Parse</c> repairs a trailing quote, which is the same mangling anyone typing
/// <c>bertbrowser "C:\Dir\"</c> hits.
/// </para>
/// <para>
/// Removal is asymmetric with writing, and has to be: <c>Directory\shell</c> is a key other
/// installers put their own verbs under (there is a VPN one on the machine this was written on),
/// so unregistering deletes the <c>open</c> subtree and the one <c>(Default)</c> value this app
/// wrote, never the <c>shell</c> key itself.
/// </para>
/// </remarks>
public static class FolderHandlerRegistration
{
    /// <summary>The program a registration must name to be recognised as this app's.</summary>
    public const string ExecutableName = "BertBrowser.exe";

    /// <summary>The verb, and the name of the subkey holding it.</summary>
    public const string OpenVerb = "open";

    /// <summary>Matches what <c>Folder\shell\open</c> already carries. <c>Document</c> means the
    /// shell invokes the verb once per selected item rather than once for the whole selection,
    /// which is what the single-instance hand-off is shaped for.</summary>
    public const string MultiSelectModel = "Document";

    /// <summary>
    /// Written <b>empty</b> on each command key, and the registration does nothing without it.
    /// </summary>
    /// <remarks>
    /// <c>Directory</c> and <c>Drive</c> derive from the <c>Folder</c> class, whose
    /// <c>shell\open\command</c> carries <c>DelegateExecute = {11dbb47c-a525-400b-9e80-a54615a090c0}</c>
    /// (<c>CLSID_ExecuteFolder</c>, in <c>ExplorerFrame.dll</c>). A <c>DelegateExecute</c> that
    /// resolves makes the shell instantiate that COM object and <b>ignore the command line
    /// entirely</b> — so an inherited one is enough to send every folder to Explorer while this
    /// registration sits there looking perfect. Blanking it on our own command key is how that
    /// inheritance is cut; it is the same empty-string shadow every other third-party file manager
    /// writes, and the reason they all write it.
    /// </remarks>
    public const string DelegateExecute = "DelegateExecute";

    public const string DirectoryShellKey = @"Software\Classes\Directory\shell";
    public const string DriveShellKey = @"Software\Classes\Drive\shell";

    public static string DirectoryOpenKey => DirectoryShellKey + "\\" + OpenVerb;
    public static string DriveOpenKey => DriveShellKey + "\\" + OpenVerb;
    public static string DirectoryCommandKey => DirectoryOpenKey + @"\command";
    public static string DriveCommandKey => DriveOpenKey + @"\command";

    /// <summary>
    /// Opening from the shell is always a <b>new tab</b>.
    /// </summary>
    /// <remarks>
    /// Double-clicking a folder in Explorer gets you a window showing that folder; it does not
    /// retarget a window you were already using. Retargeting the active tab loses whatever was in
    /// it, which is the wrong trade when the user asked to look at something new. The flag goes on
    /// the registration rather than into <c>OpenRequestAsync</c> so that
    /// <c>bertbrowser &lt;path&gt;</c> typed at a prompt keeps its documented meaning — the shell's
    /// intent is expressed by the command line the shell was given.
    /// </remarks>
    public const string NewTabFlag = "--new-tab";

    /// <summary>
    /// Everything the command line carries after the program. <c>%1</c> is the folder or drive
    /// path, quoted because folder names routinely contain spaces.
    /// </summary>
    /// <remarks>
    /// Named separately so <see cref="FolderHandlerRules"/> can tell a registration written by an
    /// older build — right program, outdated arguments — from a current one, and let the startup
    /// repair bring it up to date. Comparing only the program would leave a registration that still
    /// launches the app but no longer does what this version means by it.
    /// </remarks>
    public static string ArgumentTail { get; } = NewTabFlag + " \"%1\"";

    /// <summary>
    /// The command line the shell runs. The program is quoted for the same reason the argument is —
    /// the install path sits under a profile directory that may contain spaces.
    /// </summary>
    public static string CommandFor(string executablePath) =>
        "\"" + executablePath + "\" " + ArgumentTail;

    /// <summary>
    /// Everything the registration writes, for one executable path, <b>in the order it must be
    /// written</b>.
    /// </summary>
    /// <remarks>
    /// The command comes first, and that ordering is load-bearing. Creating the <c>open</c> key
    /// registers a verb; setting its command is what makes the verb usable. A write interrupted
    /// between the two leaves a verb the shell can select and cannot run, so the deepest key —
    /// which brings its parent into existence with it — is written first and the decoration after.
    /// The caller rolls back anything partial on top of that.
    /// </remarks>
    public static IReadOnlyList<ShellRegistryValue> ValuesFor(string executablePath)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(executablePath);

        var command = CommandFor(executablePath);
        return
        [
            new ShellRegistryValue(DirectoryCommandKey, "", command),
            new ShellRegistryValue(DirectoryCommandKey, DelegateExecute, ""),
            new ShellRegistryValue(DirectoryOpenKey, "MultiSelectModel", MultiSelectModel),
            new ShellRegistryValue(DriveCommandKey, "", command),
            new ShellRegistryValue(DriveCommandKey, DelegateExecute, ""),
            new ShellRegistryValue(DriveOpenKey, "MultiSelectModel", MultiSelectModel),
        ];
    }

    /// <summary>
    /// The default-verb values, kept apart from <see cref="ValuesFor"/> because they must be
    /// written <b>last and only once the verb they name has been read back from the registry</b>.
    /// Naming a verb that is not there hands the double-click to whatever third-party verb
    /// enumerates first. See the remarks on this class.
    /// </summary>
    public static IReadOnlyList<ShellRegistryValue> GuardValues { get; } =
    [
        new ShellRegistryValue(DirectoryShellKey, "", OpenVerb),
        new ShellRegistryValue(DriveShellKey, "", OpenVerb),
    ];

    /// <summary>The command keys whose default value must read back correctly before
    /// <see cref="GuardValues"/> may be written.</summary>
    public static IReadOnlyList<string> CommandKeys { get; } = [DirectoryCommandKey, DriveCommandKey];

    /// <summary>Keys deleted whole when unregistering — this app's verb and everything under it.</summary>
    public static IReadOnlyList<string> KeysToRemove { get; } =
    [
        DirectoryShellKey + "\\" + OpenVerb,
        DriveShellKey + "\\" + OpenVerb,
    ];

    /// <summary>Values deleted when unregistering, on keys that must survive because other
    /// programs keep their own verbs under them.</summary>
    public static IReadOnlyList<ShellRegistryValueRef> ValuesToRemove { get; } =
    [
        new ShellRegistryValueRef(DirectoryShellKey, "", OpenVerb),
        new ShellRegistryValueRef(DriveShellKey, "", OpenVerb),
    ];
}
