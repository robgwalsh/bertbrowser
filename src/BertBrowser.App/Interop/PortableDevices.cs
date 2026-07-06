using System.Diagnostics;
using System.Threading;

namespace BertBrowser.App.Interop;

/// <summary>A non-filesystem device under "This PC" — a phone, camera, or media player
/// connected over MTP/PTP. Its contents live in the shell namespace, not on a drive
/// letter, so the path-based file list can't browse it; we open it in Explorer instead.</summary>
public sealed record PortableDevice(string Name, string ShellPath);

/// <summary>
/// Enumerates MTP/PTP portable devices via the Shell.Application automation object.
/// COM is late-bound (no interop assembly) and driven on a dedicated STA thread, since
/// the shell folder objects are apartment-sensitive and enumeration can block on a
/// slow/waking device.
/// </summary>
public static class PortableDevices
{
    private const int SsfDrives = 17; // ssfDRIVES — the "This PC" shell folder.

    public static IReadOnlyList<PortableDevice> Enumerate()
    {
        var devices = new List<PortableDevice>();
        var thread = new Thread(() =>
        {
            try { EnumerateCore(devices); }
            catch { /* no shell / access issues: just show no devices */ }
        })
        {
            IsBackground = true,
            Name = "MTP device enumeration",
        };
        thread.SetApartmentState(ApartmentState.STA);
        thread.Start();
        // A stuck device must never hang the sidebar; cap the wait and move on.
        thread.Join(TimeSpan.FromSeconds(5));
        return devices;
    }

    private static void EnumerateCore(List<PortableDevice> devices)
    {
        var shellType = Type.GetTypeFromProgID("Shell.Application");
        if (shellType is null) return;

        dynamic? shell = Activator.CreateInstance(shellType);
        if (shell is null) return;

        try
        {
            dynamic thisPc = shell.NameSpace(SsfDrives);
            dynamic items = thisPc.Items();
            foreach (dynamic item in items)
            {
                // Portable devices are folders that don't map to the file system
                // (drives, and known-folder shortcuts like Desktop, are filesystem-backed).
                if (item.IsFolder && !item.IsFileSystem)
                {
                    string name = item.Name;
                    string path = item.Path;
                    if (!string.IsNullOrEmpty(name))
                        devices.Add(new PortableDevice(name, path ?? ""));
                }
            }
        }
        finally
        {
            System.Runtime.InteropServices.Marshal.FinalReleaseComObject(shell);
        }
    }

    /// <summary>Opens the device in a Windows Explorer window.</summary>
    public static void OpenInExplorer(PortableDevice device)
    {
        if (device.ShellPath.Length == 0) return;
        Process.Start(new ProcessStartInfo("explorer.exe", $"\"{device.ShellPath}\"")
        {
            UseShellExecute = true,
        });
    }
}
