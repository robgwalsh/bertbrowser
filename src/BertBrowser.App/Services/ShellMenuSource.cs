using System.Runtime.InteropServices;
using BertBrowser.App.Interop;
using BertBrowser.Core.Services.ShellMenu;
using Microsoft.Win32;

namespace BertBrowser.App.Services;

/// <summary>
/// The real <see cref="IShellMenuSource"/>: the registry for what is registered, Core for what
/// that means, and one <see cref="ShellContextMenuHandler"/> per COM handler for the items.
/// </summary>
/// <remarks>
/// <para>
/// Composed per extension rather than asked of the shell whole, for the reasons on
/// <see cref="ShellMenuRules"/>. The order is the catalog's: static verbs, then handlers.
/// </para>
/// <para>
/// A static verb is started through <see cref="IProcessLauncher"/>, which keeps the app's one
/// <c>Process.Start</c> where it is. A COM item is started by the handler itself inside
/// <c>InvokeCommand</c> — the one place in the app something other than the launcher starts a
/// program, and unavoidably so: what 7-Zip does when clicked is 7-Zip's to decide.
/// </para>
/// </remarks>
public sealed class ShellMenuSource : IShellMenuSource
{
    private const int UserCancelled = unchecked((int)0x800704C7); // HRESULT_FROM_WIN32(ERROR_CANCELLED)

    private sealed record VerbCommand(ShellExtension Extension, ShellVerbRegistration Verb);

    private readonly AppSettings _settings;
    private readonly IProcessLauncher _launcher;

    /// <summary>Read once: Explorer reads it at load, and a handler does not become blocked while
    /// the app runs.</summary>
    private readonly Lazy<IReadOnlySet<Guid>> _blocked = new(ShellExtensionRegistry.Blocked);

    public ShellMenuSource(AppSettings settings, IProcessLauncher launcher)
    {
        _settings = settings;
        _launcher = launcher;
    }

    public ShellMenuSession? Open(IReadOnlyList<ShellMenuTarget> targets, ShellMenuContext context, string folder)
    {
        if (!_settings.ShowShellExtensions || folder.Length == 0) return null;

        var families = ShellMenuKeys.For(targets, context, ShellExtensionRegistry.TypeOf);
        if (families.Count == 0) return null;

        var (handlers, verbs) = ShellExtensionRegistry.Read(families);
        var visible = ShellMenuRules.Visible(
            ShellMenuRules.Catalog(handlers, verbs, _blocked.Value), enabled: true, _settings.HiddenShellExtensions);
        if (visible.Count == 0) return null;

        var owned = new List<IDisposable>();
        var entries = new List<ShellMenuEntry>();
        ShellSelection? selection = null;
        var selectionTried = false;

        foreach (var extension in visible)
        {
            if (extension.Kind == ShellExtensionKind.Verb)
            {
                if (VerbFor(extension, families) is not { } verb) continue;

                entries.Add(ShellMenuEntry.Item(
                    ShellMenuRules.Header(extension.Name),
                    new VerbCommand(extension, verb),
                    icon: ShellMenuIcons.FromIconResource(verb.Icon)));
                continue;
            }

            if (!selectionTried)
            {
                selectionTried = true;
                selection = ShellSelection.Create(
                    folder,
                    context == ShellMenuContext.Items ? targets.Select(t => t.FullPath).ToList() : []);
                if (selection is not null) owned.Add(selection);
            }

            if (selection is null || extension.Clsid is not { } clsid) continue;

            var handler = CreateHandler(clsid, selection, extension.Families[0]);
            if (handler is null) continue;

            owned.Add(handler);
            entries.AddRange(handler.Query(ShellMenuIcons.FromMenuBitmap));
        }

        var tidy = ShellMenuRules.Tidy(entries);
        if (tidy.Count == 0)
        {
            ReleaseAll(owned);
            return null;
        }

        return new ShellMenuSession(
            tidy,
            (entry, owner) => Invoke(entry, owner, targets, context, folder),
            () => ReleaseAll(owned));
    }

    public Task<IReadOnlyList<ShellExtension>> CatalogAsync() => Task.Run(() =>
    {
        var (handlers, verbs) = ShellExtensionRegistry.Read(ShellExtensionRegistry.AllFamilies());
        return ShellMenuRules.Catalog(handlers, verbs, _blocked.Value);
    });

    /// <summary>The handler is told which key it was found under, as Explorer tells it; a few read
    /// their own settings from there. The key is held open only across initialisation.</summary>
    private static ShellContextMenuHandler? CreateHandler(Guid clsid, ShellSelection selection, string family)
    {
        RegistryKey? key = null;
        try
        {
            try
            {
                key = Registry.ClassesRoot.OpenSubKey(family, writable: false);
            }
            catch (Exception ex) when (ex is System.Security.SecurityException or UnauthorizedAccessException or IOException)
            {
            }

            return ShellContextMenuHandler.Create(
                clsid, selection, key?.Handle.DangerousGetHandle() ?? IntPtr.Zero);
        }
        finally
        {
            key?.Dispose();
        }
    }

    private static ShellVerbRegistration? VerbFor(ShellExtension extension, IReadOnlyList<string> families)
    {
        foreach (var family in families)
        {
            var match = extension.Verbs.FirstOrDefault(v =>
                string.Equals(v.Family, family, StringComparison.OrdinalIgnoreCase));
            if (match is not null) return match;
        }

        return extension.Verbs.Count > 0 ? extension.Verbs[0] : null;
    }

    private string? Invoke(
        ShellMenuEntry entry, IntPtr owner, IReadOnlyList<ShellMenuTarget> targets,
        ShellMenuContext context, string folder)
    {
        switch (entry.Command)
        {
            case ShellContextMenuHandler.Command com:
            {
                var hr = com.Handler.Invoke(com.Id, owner, folder);
                if (hr >= 0 || hr == UserCancelled) return null;

                var reason = Marshal.GetExceptionForHR(hr)?.Message ?? $"0x{hr:X8}";
                return $"'{Plain(entry.Header)}' failed: {reason}";
            }

            case VerbCommand verb:
                return RunVerb(verb, targets, context, folder);

            default:
                return null;
        }
    }

    /// <summary>Once per target, the way a custom command runs; the folder itself for a
    /// background verb.</summary>
    private string? RunVerb(
        VerbCommand verb, IReadOnlyList<ShellMenuTarget> targets, ShellMenuContext context, string folder)
    {
        var name = verb.Extension.Name;
        var run = context == ShellMenuContext.Background || targets.Count == 0
            ? [new ShellMenuTarget(folder, true)]
            : targets;

        foreach (var target in run)
        {
            var command = StaticVerbCommand.Resolve(verb.Verb.Command, target.FullPath, folder, File.Exists);
            if (command is null)
                return $"'{name}' could not be run: its program was not found.";

            var message = _launcher.Launch(
                command.Executable, command.Arguments,
                StaticVerbCommand.WorkingDirectoryFor(target, context, folder));
            if (message is not null) return $"'{name}' failed: {message}";
        }

        return null;
    }

    /// <summary>A header back to plain text for a message: a doubled underscore was a literal
    /// one, a single underscore marked the access key.</summary>
    private static string Plain(string header)
    {
        const char literal = '';
        return header.Replace("__", literal.ToString()).Replace("_", "").Replace(literal, '_');
    }

    private static void ReleaseAll(List<IDisposable> owned)
    {
        // Handlers before the selection they were initialised with, i.e. newest first.
        for (var i = owned.Count - 1; i >= 0; i--) owned[i].Dispose();
        owned.Clear();
    }
}
