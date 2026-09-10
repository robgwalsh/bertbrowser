using BertBrowser.Core.Services.ShellMenu;
using Microsoft.Win32;

namespace BertBrowser.App.Interop;

/// <summary>
/// Reads the registrations Explorer builds a right-click menu from: COM context-menu handlers
/// under <c>shellex\ContextMenuHandlers</c> and static verbs under <c>shell</c>, per key family.
/// </summary>
/// <remarks>
/// <para>
/// <b>Read-only</b>, in the sense <see cref="ShellNewRegistry"/> is: nothing here creates, sets or
/// deletes a value. And raw, in the same sense: which registrations become a row, what they are
/// called and which are never shown is <see cref="ShellMenuRules"/>' business, in Core, where a
/// project that cannot open a key can test it.
/// </para>
/// <para>
/// <c>HKEY_CLASSES_ROOT</c> is the merged view of the machine's and the user's classes, which is
/// the view Explorer takes too — so a per-user install of an extension shows up here.
/// </para>
/// </remarks>
internal static class ShellExtensionRegistry
{
    private const string BlockedKey = @"SOFTWARE\Microsoft\Windows\CurrentVersion\Shell Extensions\Blocked";

    /// <summary>Every handler and verb under the given families, in registry order. Never throws:
    /// a key that cannot be read contributes nothing.</summary>
    public static (IReadOnlyList<ShellHandlerRegistration> Handlers, IReadOnlyList<ShellVerbRegistration> Verbs)
        Read(IEnumerable<string> families)
    {
        var handlers = new List<ShellHandlerRegistration>();
        var verbs = new List<ShellVerbRegistration>();

        try
        {
            using var classes = Registry.ClassesRoot;
            foreach (var family in families)
            {
                try
                {
                    using var key = classes.OpenSubKey(family, writable: false);
                    if (key is null) continue;

                    ReadHandlers(classes, key, family, handlers);
                    ReadVerbs(key, family, verbs);
                }
                catch (Exception ex) when (IsRegistryFailure(ex))
                {
                }
            }
        }
        catch (Exception ex) when (IsRegistryFailure(ex))
        {
        }

        return (handlers, verbs);
    }

    /// <summary>What an extension (".txt") maps to, or null when the registry has nothing.</summary>
    public static ShellFileType? TypeOf(string extension)
    {
        try
        {
            using var classes = Registry.ClassesRoot;
            using var key = classes.OpenSubKey(extension, writable: false);
            if (key is null) return null;

            return new ShellFileType(
                key.GetValue(null) as string,
                key.GetValue("PerceivedType") as string);
        }
        catch (Exception ex) when (IsRegistryFailure(ex))
        {
            return null;
        }
    }

    /// <summary>The handlers Windows itself refuses to load, from both hives.</summary>
    public static IReadOnlySet<Guid> Blocked()
    {
        var blocked = new HashSet<Guid>();

        foreach (var hive in new[] { Registry.LocalMachine, Registry.CurrentUser })
        {
            try
            {
                using var key = hive.OpenSubKey(BlockedKey, writable: false);
                if (key is null) continue;

                foreach (var name in key.GetValueNames())
                {
                    if (Guid.TryParse(name, out var clsid)) blocked.Add(clsid);
                }
            }
            catch (Exception ex) when (IsRegistryFailure(ex))
            {
            }
        }

        return blocked;
    }

    /// <summary>
    /// Every family the Settings page should scan: the generic ones, then every ProgID an extension
    /// maps to and every <c>SystemFileAssociations</c> entry. Thousands of cheap reads — the caller
    /// runs it off the UI thread.
    /// </summary>
    public static IReadOnlyList<string> AllFamilies()
    {
        var families = new List<string>(ShellMenuKeys.Generic);
        var seen = new HashSet<string>(families, StringComparer.OrdinalIgnoreCase);

        try
        {
            using var classes = Registry.ClassesRoot;
            foreach (var name in classes.GetSubKeyNames())
            {
                if (name.Length < 2 || name[0] != '.') continue;

                try
                {
                    using var extension = classes.OpenSubKey(name, writable: false);
                    if (extension?.GetValue(null) is string { Length: > 0 } progId && seen.Add(progId))
                        families.Add(progId);
                }
                catch (Exception ex) when (IsRegistryFailure(ex))
                {
                }
            }

            using var associations = classes.OpenSubKey(ShellMenuKeys.SystemFileAssociations, writable: false);
            if (associations is not null)
            {
                foreach (var name in associations.GetSubKeyNames())
                {
                    var family = ShellMenuKeys.SystemFileAssociations + @"\" + name;
                    if (seen.Add(family)) families.Add(family);
                }
            }
        }
        catch (Exception ex) when (IsRegistryFailure(ex))
        {
        }

        return families;
    }

    private static void ReadHandlers(
        RegistryKey classes, RegistryKey family, string familyName, List<ShellHandlerRegistration> into)
    {
        using var handlers = family.OpenSubKey(@"shellex\ContextMenuHandlers", writable: false);
        if (handlers is null) return;

        foreach (var name in handlers.GetSubKeyNames())
        {
            try
            {
                using var handler = handlers.OpenSubKey(name, writable: false);
                if (handler is null) continue;

                // The CLSID is the default value; some installers leave that blank and make the
                // key name the CLSID instead.
                if (!Guid.TryParse((handler.GetValue(null) as string)?.Trim(), out var clsid) &&
                    !Guid.TryParse(name.Trim(), out clsid))
                    continue;

                into.Add(new ShellHandlerRegistration(familyName, name, clsid, ClassNameOf(classes, clsid)));
            }
            catch (Exception ex) when (IsRegistryFailure(ex))
            {
            }
        }
    }

    private static string? ClassNameOf(RegistryKey classes, Guid clsid)
    {
        try
        {
            using var key = classes.OpenSubKey(@"CLSID\" + clsid.ToString("B"), writable: false);
            return key?.GetValue(null) as string;
        }
        catch (Exception ex) when (IsRegistryFailure(ex))
        {
            return null;
        }
    }

    private static void ReadVerbs(RegistryKey family, string familyName, List<ShellVerbRegistration> into)
    {
        using var shell = family.OpenSubKey("shell", writable: false);
        if (shell is null) return;

        foreach (var verb in shell.GetSubKeyNames())
        {
            try
            {
                using var key = shell.OpenSubKey(verb, writable: false);
                if (key is null) continue;

                var values = new HashSet<string>(key.GetValueNames(), StringComparer.OrdinalIgnoreCase);

                string? command = null;
                var delegateExecute = false;
                using (var commandKey = key.OpenSubKey("command", writable: false))
                {
                    if (commandKey is not null)
                    {
                        // GetValue already expands a REG_EXPAND_SZ; a plain string holding
                        // %ProgramFiles% is expanded here so the parser never sees one.
                        if (commandKey.GetValue(null) is string { Length: > 0 } raw)
                            command = Environment.ExpandEnvironmentVariables(raw);
                        delegateExecute = commandKey.GetValueNames()
                            .Contains("DelegateExecute", StringComparer.OrdinalIgnoreCase);
                    }
                }

                into.Add(new ShellVerbRegistration(
                    familyName,
                    verb,
                    DisplayNameOf(key, values),
                    key.GetValue("Icon") as string,
                    command,
                    Extended: values.Contains("Extended"),
                    LegacyDisable: values.Contains("LegacyDisable"),
                    ProgrammaticOnly: values.Contains("ProgrammaticAccessOnly"),
                    HasDelegateExecute: delegateExecute,
                    HasSubCommands: values.Contains("SubCommands") || values.Contains("ExtendedSubCommandsKey")));
            }
            catch (Exception ex) when (IsRegistryFailure(ex))
            {
            }
        }
    }

    /// <summary><c>MUIVerb</c> first, then the key's default value; either may be an
    /// <c>@dll,-id</c> reference, resolved the way the New menu's labels are.</summary>
    private static string? DisplayNameOf(RegistryKey verb, HashSet<string> values)
    {
        var candidate = values.Contains("MUIVerb")
            ? verb.GetValue("MUIVerb") as string
            : verb.GetValue(null) as string;

        if (string.IsNullOrWhiteSpace(candidate)) return null;

        return candidate.StartsWith('@')
            ? ShellNewRegistry.LoadIndirectString(candidate)
            : candidate;
    }

    private static bool IsRegistryFailure(Exception ex) =>
        ex is System.Security.SecurityException or UnauthorizedAccessException or IOException;
}
