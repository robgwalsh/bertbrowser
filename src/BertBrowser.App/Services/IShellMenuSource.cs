using BertBrowser.Core.Services.ShellMenu;

namespace BertBrowser.App.Services;

/// <summary>
/// Other programs' right-click entries — 7-Zip, Git, TortoiseSVN — for a selection or a folder.
/// </summary>
/// <remarks>
/// An interface for the reason <see cref="IShellNewCatalog"/> is one, and one more: the real
/// source loads other people's code into this process and invoking an entry starts whatever that
/// code starts. The harness gets a canned one, so a scripted run photographs a fixed menu and
/// launches nothing.
/// </remarks>
public interface IShellMenuSource
{
    /// <summary>
    /// The entries for one opening of a menu, or null when there are none — the setting is off,
    /// nothing is registered, or every registered extension declined. The caller owns the session
    /// for as long as its items are on screen and disposes it after the menu has closed.
    /// </summary>
    /// <param name="targets">The selected rows, in list order; empty for a folder background.</param>
    /// <param name="folder">The folder the menu is over — the working directory of anything run.</param>
    ShellMenuSession? Open(IReadOnlyList<ShellMenuTarget> targets, ShellMenuContext context, string folder);

    /// <summary>Every extension registered on this machine, for the Settings checklist. Off the UI
    /// thread: it walks the whole of HKEY_CLASSES_ROOT.</summary>
    Task<IReadOnlyList<ShellExtension>> CatalogAsync();
}

/// <summary>
/// One opening of the shell's part of a menu: the entries to show and the means to run one.
/// </summary>
/// <remarks>
/// Owns the COM objects, menus and PIDLs behind the entries, which is why it is disposable and why
/// the view disposes it only after the menu has closed, deferred through the dispatcher: the click
/// that invokes an entry has to have run first, and a handler released before its
/// <c>InvokeCommand</c> is a call into freed memory.
/// </remarks>
public sealed class ShellMenuSession : IDisposable
{
    private readonly Func<ShellMenuEntry, IntPtr, string?> _invoke;
    private readonly Action _release;
    private bool _disposed;

    public ShellMenuSession(
        IReadOnlyList<ShellMenuEntry> entries,
        Func<ShellMenuEntry, IntPtr, string?> invoke,
        Action release)
    {
        Entries = entries;
        _invoke = invoke;
        _release = release;
    }

    public IReadOnlyList<ShellMenuEntry> Entries { get; }

    /// <summary>Runs an entry. Null on success, else a status-bar message — the channel
    /// <see cref="IProcessLauncher.Launch"/> reports on, so the two read alike.</summary>
    /// <param name="ownerWindow">The window any dialog the extension shows should be owned by.</param>
    public string? Invoke(ShellMenuEntry entry, IntPtr ownerWindow) =>
        _disposed ? "That menu has already closed." : _invoke(entry, ownerWindow);

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        _release();
    }
}
