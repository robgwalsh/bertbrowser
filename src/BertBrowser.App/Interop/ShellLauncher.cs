using System.Runtime.InteropServices;
using System.Threading;

namespace BertBrowser.App.Interop;

/// <summary>How a launch through the desktop shell ended.</summary>
internal enum ShellLaunchResult
{
    /// <summary>Handed to the shell, which started it as the logged-on user.</summary>
    Launched,

    /// <summary>There is no reachable desktop shell — explorer is not running, or this session has
    /// a different one. Nothing was started, so the caller may safely offer an alternative.</summary>
    Unavailable,

    /// <summary>The shell was reached but did not finish in time. <b>Nothing may be retried after
    /// this</b>: the request may yet complete, and a second attempt would start it twice.</summary>
    Unresponsive,
}

/// <summary>
/// Launches something the way the logged-on user would, from a process that is running as
/// administrator.
/// </summary>
/// <remarks>
/// <para>
/// BertBrowser holds an administrator token because reading the MFT needs one. A child started with
/// <c>Process.Start</c> inherits that token, with no prompt — so double-clicking a downloaded
/// program would run it as administrator. This class is what stops that.
/// </para>
/// <para>
/// The trick is to not do the launching. <c>explorer.exe</c> is already running at the user's own
/// (medium) integrity level, and it publishes its automation object system-wide as
/// <c>ShellWindows</c>. Reaching that object gets us an <c>IShellDispatch2</c> living <b>in
/// explorer's process</b>; asking it to <c>ShellExecute</c> makes explorer the parent, so the child
/// gets explorer's token rather than ours.
/// </para>
/// <para>
/// The same indirection is what restores the UAC prompt. Asking for the <c>runas</c> verb from
/// <i>this</i> process would elevate silently — we already hold the token, so there is nothing to
/// consent to. Asked from medium-integrity explorer, it is a real elevation request and Windows
/// shows the consent dialog, exactly as it does for Explorer's own "Run as administrator".
/// </para>
/// <para>
/// COM here is a mixture of the two styles already in this folder, for the reason each exists:
/// typed <c>[ComImport]</c> declarations (as <see cref="ShellThumbnails"/> uses) for the navigation
/// chain, because those are <c>IUnknown</c> vtables and the slots have to line up; and late-bound
/// <c>dynamic</c> (as <see cref="PortableDevices"/> uses) for the last two hops, because those are
/// <c>IDispatch</c> and typing them would buy nothing.
/// </para>
/// </remarks>
internal static class ShellLauncher
{
    // The shell's automation object, as published by the running explorer.exe.
    private static readonly Guid CLSID_ShellWindows = new("9BA05972-F6A8-11CF-A442-00A0C90A8F39");
    private static readonly Guid SID_STopLevelBrowser = new("4C96BE40-915C-11CF-99D3-00AA004AE837");
    private static readonly Guid IID_IShellBrowser = new("000214E2-0000-0000-C000-000000000046");
    private static readonly Guid IID_IDispatch = new("00020400-0000-0000-C000-000000000046");

    private const int SWC_DESKTOP = 8;        // the desktop's own shell window
    private const int SWFO_NEEDDISPATCH = 1;  // give us the IDispatch, not just the HWND
    private const uint SVGIO_BACKGROUND = 0;  // the view itself rather than a selected item
    private const int SW_SHOWNORMAL = 1;

    /// <summary>A hung shell must not hang the UI, so the whole chain runs with a deadline — the
    /// same bargain <see cref="PortableDevices.Enumerate"/> makes with a waking device.</summary>
    private static readonly TimeSpan Deadline = TimeSpan.FromSeconds(5);

    [DllImport("user32.dll")]
    private static extern IntPtr GetShellWindow();

    [DllImport("user32.dll")]
    private static extern int GetWindowThreadProcessId(IntPtr hWnd, out int lpdwProcessId);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool AllowSetForegroundWindow(int dwProcessId);

    /// <summary>
    /// Asks the desktop shell to launch <paramref name="file"/> as the logged-on user. Never
    /// throws, so the caller can decide what to do rather than discovering it through an exception.
    /// </summary>
    /// <param name="verb">The shell verb, or null for the default one. <c>"runas"</c> asks for
    /// elevation, which from medium-integrity explorer means a genuine UAC prompt.</param>
    public static ShellLaunchResult ShellExecuteAsUser(
        string file, string? arguments, string? workingDirectory, string? verb, out string? error)
    {
        var succeeded = false;
        string? failure = null;

        // Explorer is about to start a process on our behalf, and the foreground belongs to us.
        // Without this hand-over the new window opens *behind* BertBrowser — and the UAC prompt,
        // which explorer owns rather than us, can end up behind it too.
        GrantForegroundToShell();

        var thread = new Thread(() =>
        {
            try
            {
                succeeded = Execute(file, arguments, workingDirectory, verb, out failure);
            }
            catch (Exception ex)
            {
                failure = ex.Message;
            }
        })
        {
            IsBackground = true,
            Name = "Shell launch",
        };

        // Shell objects are apartment-sensitive; this has to be STA.
        thread.SetApartmentState(ApartmentState.STA);
        thread.Start();

        // A timeout is deliberately its own answer rather than a failure. The worker may still be
        // inside ShellExecute, so "it didn't work, shall I run it as administrator instead?" could
        // start the thing twice. The caller reports and stops.
        if (!thread.Join(Deadline))
        {
            error = "Windows Explorer did not respond.";
            return ShellLaunchResult.Unresponsive;
        }

        error = succeeded ? null : failure ?? "Windows Explorer could not be reached.";
        return succeeded ? ShellLaunchResult.Launched : ShellLaunchResult.Unavailable;
    }

    /// <summary>Lets the shell's process take the foreground, which it otherwise may not while we
    /// hold it. Best-effort: failing only costs z-order.</summary>
    private static void GrantForegroundToShell()
    {
        try
        {
            var shellWindow = GetShellWindow();
            if (shellWindow == IntPtr.Zero) return;
            if (GetWindowThreadProcessId(shellWindow, out var shellProcessId) == 0) return;
            AllowSetForegroundWindow(shellProcessId);
        }
        catch (EntryPointNotFoundException)
        {
        }
    }

    private static bool Execute(
        string file, string? arguments, string? workingDirectory, string? verb, out string? error)
    {
        error = null;

        var type = Type.GetTypeFromCLSID(CLSID_ShellWindows);
        if (type is null)
        {
            error = "The Windows shell is not available.";
            return false;
        }

        object? windows = null;
        try
        {
            windows = Activator.CreateInstance(type);
            if (windows is not IShellWindows shellWindows)
            {
                error = "The Windows shell is not available.";
                return false;
            }

            // Empty VARIANTs: we want the desktop, which SWC_DESKTOP already names.
            object location = Type.Missing;
            object root = Type.Missing;
            var desktop = shellWindows.FindWindowSW(
                ref location, ref root, SWC_DESKTOP, out _, SWFO_NEEDDISPATCH);

            if (desktop is not IComServiceProvider provider)
            {
                error = "The desktop window is not available.";
                return false;
            }

            provider.QueryService(SID_STopLevelBrowser, IID_IShellBrowser, out var browserObject);
            if (browserObject is not IShellBrowser browser)
            {
                error = "The desktop window is not available.";
                return false;
            }

            browser.QueryActiveShellView(out var view);
            view.GetItemObject(SVGIO_BACKGROUND, IID_IDispatch, out var folderView);

            // IDispatch from here down: .Application is an IShellDispatch2 living in explorer, and
            // its ShellExecute is the whole point — explorer performs the launch, so explorer's
            // token is the one the child inherits.
            dynamic shell = ((dynamic)folderView).Application;
            shell.ShellExecute(
                file,
                arguments ?? "",
                workingDirectory ?? "",
                verb ?? "",
                SW_SHOWNORMAL);

            return true;
        }
        catch (Exception ex) when (ex is COMException or InvalidCastException or NotSupportedException
                                      or MissingMemberException or UnauthorizedAccessException)
        {
            error = ex.Message;
            return false;
        }
        finally
        {
            if (windows is not null) Marshal.FinalReleaseComObject(windows);
        }
    }

    // --- COM declarations ---
    //
    // Every method ahead of the one we call has to be declared so the vtable slots line up. The
    // placeholders are never invoked; only their position matters.

    [ComImport, Guid("85CB6900-4D95-11CF-960C-0080C7F4EE85"),
     InterfaceType(ComInterfaceType.InterfaceIsDual)]
    private interface IShellWindows
    {
        int Count { get; }
        void Item();
        void _NewEnum();
        void Register();
        void RegisterPending();
        void Revoke();
        void OnNavigate();
        void OnActivated();

        [return: MarshalAs(UnmanagedType.IDispatch)]
        object? FindWindowSW(
            [MarshalAs(UnmanagedType.Struct)] ref object pvarLoc,
            [MarshalAs(UnmanagedType.Struct)] ref object pvarLocRoot,
            int swClass,
            out int pHWND,
            int swfwOptions);
    }

    /// <summary>Named for COM's <c>IServiceProvider</c>; renamed here only because
    /// <see cref="System.IServiceProvider"/> already owns that name in C#.</summary>
    [ComImport, Guid("6D5140C1-7436-11CE-8034-00AA006009FA"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IComServiceProvider
    {
        void QueryService(in Guid guidService, in Guid riid,
            [MarshalAs(UnmanagedType.IUnknown)] out object ppvObject);
    }

    [ComImport, Guid("000214E2-0000-0000-C000-000000000046"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IShellBrowser
    {
        // IOleWindow
        void GetWindow(out IntPtr phwnd);
        void ContextSensitiveHelp([MarshalAs(UnmanagedType.Bool)] bool fEnterMode);

        // IShellBrowser, in vtable order.
        void InsertMenusSB();
        void SetMenuSB();
        void RemoveMenusSB();
        void SetStatusTextSB();
        void EnableModelessSB();
        void TranslateAcceleratorSB();
        void BrowseObject();
        void GetViewStateStream();
        void GetControlWindow();
        void SendControlMsg();

        void QueryActiveShellView([MarshalAs(UnmanagedType.Interface)] out IShellView ppshv);
    }

    [ComImport, Guid("000214E3-0000-0000-C000-000000000046"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IShellView
    {
        // IOleWindow
        void GetWindow(out IntPtr phwnd);
        void ContextSensitiveHelp([MarshalAs(UnmanagedType.Bool)] bool fEnterMode);

        // IShellView, in vtable order.
        void TranslateAccelerator();
        void EnableModeless();
        void UIActivate();
        void Refresh();
        void CreateViewWindow();
        void DestroyViewWindow();
        void GetCurrentInfo();
        void AddPropertySheetPages();
        void SaveViewState();
        void SelectItem();

        void GetItemObject(uint uItem, in Guid riid,
            [MarshalAs(UnmanagedType.IUnknown)] out object ppv);
    }
}
