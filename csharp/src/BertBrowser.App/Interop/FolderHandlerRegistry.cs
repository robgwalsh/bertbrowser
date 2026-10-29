using BertBrowser.Core.Services.ShellIntegration;
using Microsoft.Win32;

namespace BertBrowser.App.Interop;

/// <summary>
/// Reads and writes the <c>Directory</c> and <c>Drive</c> open verbs that decide whether Windows
/// opens a folder in BertBrowser or in Explorer.
/// </summary>
/// <remarks>
/// <para>
/// <b>This is the one place BertBrowser writes to the registry</b>, and the only reason it may is
/// that the user asked for it in Settings and can take it back there. Everything about what to
/// write, what a reading means and which keys must survive removal is decided by
/// <see cref="FolderHandlerRegistration"/> and <see cref="FolderHandlerRules"/> in Core, where a
/// project that cannot open a registry key can test it. Nothing but raw values crosses that seam —
/// the same split <see cref="ShellNewRegistry"/> and <c>ShellNewImport</c> use.
/// </para>
/// <para>
/// <b>Everything is under <c>HKEY_CURRENT_USER</c>, which is what keeps this an <c>asInvoker</c>
/// feature.</b> Reads deliberately do not go through the merged <c>HKEY_CLASSES_ROOT</c> view
/// either: HKCU values mask HKLM ones at the same key, so a registration in HKCU is in effect
/// whatever HKLM says, and reading HKCU alone answers "is ours the one that wins?" without a
/// machine-wide entry making us report a rival we would in fact override.
/// </para>
/// <para>
/// Never throws. A key that cannot be read is reported as no registration, and a write that fails
/// comes back <c>false</c> for the caller to show — the settings toggle has somewhere to put that,
/// and the startup self-heal has nothing useful to do about it either way.
/// </para>
/// </remarks>
internal static class FolderHandlerRegistry
{
    /// <summary>
    /// The executable a registration should name. On a Velopack install this resolves inside
    /// <c>current\</c>, which is a fixed folder rather than a per-version one, so the path survives
    /// updates. Resolved beside this assembly the way <c>ElevatedIndexHostLauncher</c> resolves the
    /// index helper, and never by name — what runs must not be decided by <c>PATH</c>.
    /// </summary>
    public static string ExecutablePath { get; } =
        Path.Combine(AppContext.BaseDirectory, FolderHandlerRegistration.ExecutableName);

    /// <summary>What the registry currently says, classified against <see cref="ExecutablePath"/>.</summary>
    public static FolderHandlerState State() => FolderHandlerRules.Classify(Read(), ExecutablePath);

    /// <summary>
    /// Puts a live path back if this app's registration has gone stale, and does nothing at all
    /// otherwise. The rule is <see cref="FolderHandlerRules.ShouldRepair"/>, in Core, where it is
    /// tested — this is only the probe and the write.
    /// </summary>
    public static void RepairIfStale()
    {
        if (!FolderHandlerRules.ShouldRepair(Read(), ExecutablePath, File.Exists)) return;

        // Remove before writing, so a repair also clears anything the registration should not
        // contain — a masked default-verb guard above all, which re-writing the command alone would
        // leave in place.
        TryUnregister();
        TryRegister();
    }

    /// <summary>The raw values, uninterpreted.</summary>
    public static FolderHandlerReading Read()
    {
        try
        {
            return new FolderHandlerReading(
                StringValue(FolderHandlerRegistration.DirectoryShellKey),
                StringValue(FolderHandlerRegistration.DirectoryCommandKey),
                StringValue(FolderHandlerRegistration.DriveShellKey),
                StringValue(FolderHandlerRegistration.DriveCommandKey));
        }
        catch (Exception ex) when (IsRegistryFailure(ex))
        {
            return FolderHandlerReading.None;
        }
    }

    /// <summary>
    /// Points the folder and drive verbs at this executable. Idempotent — safe to press twice, and
    /// safe to call from the startup repair.
    /// </summary>
    /// <remarks>
    /// <b>All or nothing.</b> A half-written registration is not merely a feature that did not
    /// switch on — it can leave an <c>open</c> verb the shell will select and cannot run, which
    /// costs the user their folder double-click. So any failure rolls the whole thing back rather
    /// than returning false over the wreckage.
    /// </remarks>
    public static bool TryRegister()
    {
        try
        {
            if (!Write(FolderHandlerRegistration.ValuesFor(ExecutablePath)))
            {
                TryUnregister();
                return false;
            }

            // The verb has to be *there* before anything names it as the default action, and
            // "there" means read back off the registry rather than inferred from the writes above
            // having not thrown. Naming a verb that is missing is what sends a folder double-click
            // to whatever third-party verb enumerates first.
            var expected = FolderHandlerRegistration.CommandFor(ExecutablePath);
            foreach (var keyPath in FolderHandlerRegistration.CommandKeys)
            {
                if (StringValue(keyPath) == expected) continue;
                TryUnregister();
                return false;
            }

            if (!Write(FolderHandlerRegistration.GuardValues))
            {
                TryUnregister();
                return false;
            }

            return true;
        }
        catch (Exception ex) when (IsRegistryFailure(ex))
        {
            TryUnregister();
            return false;
        }
    }

    private static bool Write(IEnumerable<ShellRegistryValue> values)
    {
        foreach (var value in values)
        {
            using var key = Registry.CurrentUser.CreateSubKey(value.KeyPath, writable: true);
            if (key is null) return false;
            key.SetValue(value.ValueName, value.Data, RegistryValueKind.String);
        }

        return true;
    }

    /// <summary>
    /// Hands folders and drives back to Explorer. Deletes this app's verb subtrees and the one
    /// default-verb value it wrote, never the <c>shell</c> keys themselves — other installers keep
    /// their own verbs under those, and there is one on the machine this was written on.
    /// </summary>
    public static bool TryUnregister()
    {
        try
        {
            var removedSomething = false;

            foreach (var keyPath in FolderHandlerRegistration.KeysToRemove)
            {
                if (!Exists(keyPath)) continue;
                Registry.CurrentUser.DeleteSubKeyTree(keyPath, throwOnMissingSubKey: false);
                removedSomething = true;
            }

            foreach (var value in FolderHandlerRegistration.ValuesToRemove)
            {
                using var key = Registry.CurrentUser.OpenSubKey(value.KeyPath, writable: true);

                // Only what this app wrote. Anything else on that shared key belongs to whoever put
                // it there — including nothing at all, which is what an untouched machine looks
                // like and must stay looking like.
                if (key?.GetValue(value.ValueName) as string != value.ExpectedData) continue;

                key.DeleteValue(value.ValueName, throwOnMissingValue: false);
                removedSomething = true;
            }

            // Only tidy up after a removal that actually removed something. Unregistering when
            // nothing of ours was there must be a pure no-op — an empty `Drive\shell` is a key
            // Windows or another program may have put there, and deleting it because it happened to
            // be empty is the same mistake as deleting the `Directory\shell` other installers keep
            // their verbs under. (Found by running this against a real machine: it deleted one.)
            if (removedSomething)
                foreach (var value in FolderHandlerRegistration.ValuesToRemove)
                    PruneIfEmpty(value.KeyPath);

            return true;
        }
        catch (Exception ex) when (IsRegistryFailure(ex))
        {
            return false;
        }
    }

    private static bool Exists(string keyPath)
    {
        using var key = Registry.CurrentUser.OpenSubKey(keyPath, writable: false);
        return key is not null;
    }

    private static string? StringValue(string keyPath)
    {
        using var key = Registry.CurrentUser.OpenSubKey(keyPath, writable: false);

        // The default value, which is what the shell reads for both a verb name and a command.
        return key?.GetValue("") as string;
    }

    /// <summary>
    /// Removes a key this app created only if unregistering emptied it, so a clean removal leaves
    /// no trace — but leaves it alone the moment anything else lives there.
    /// </summary>
    private static void PruneIfEmpty(string keyPath)
    {
        try
        {
            using (var key = Registry.CurrentUser.OpenSubKey(keyPath, writable: false))
            {
                if (key is null) return;
                if (key.SubKeyCount > 0 || key.ValueCount > 0) return;
            }

            Registry.CurrentUser.DeleteSubKey(keyPath, throwOnMissingSubKey: false);
        }
        catch (Exception ex) when (IsRegistryFailure(ex))
        {
        }
    }

    private static bool IsRegistryFailure(Exception ex) =>
        ex is System.Security.SecurityException or UnauthorizedAccessException or IOException
            or ArgumentException;
}
