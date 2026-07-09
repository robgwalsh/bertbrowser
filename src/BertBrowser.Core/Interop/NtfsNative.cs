using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace BertBrowser.Core.Interop;

/// <summary>
/// Thin P/Invoke layer for reading the NTFS Master File Table and USN change journal.
/// Every entry point here needs a raw volume handle (<c>\\.\C:</c>), which the OS only
/// grants to an elevated process — the app ships with a requireAdministrator manifest.
/// Higher-level orchestration lives in <c>MftVolumeIndexer</c>; this file is purely the
/// Win32 surface plus the small structs the FSCTLs marshal.
/// </summary>
internal static class NtfsNative
{
    // --- CreateFile access / share / disposition ---
    internal const uint GenericRead = 0x80000000;
    internal const uint GenericWrite = 0x40000000;
    internal const uint FileShareRead = 0x00000001;
    internal const uint FileShareWrite = 0x00000002;
    internal const uint OpenExisting = 3;

    // --- GetDriveType results ---
    internal const uint DriveFixed = 3;

    // --- FSCTL control codes (CTL_CODE, FILE_DEVICE_FILE_SYSTEM) ---
    internal const uint FsctlQueryUsnJournal = 0x000900F4;
    internal const uint FsctlCreateUsnJournal = 0x000900E7;
    internal const uint FsctlEnumUsnData = 0x000900B3;
    internal const uint FsctlReadUsnJournal = 0x000900BB;

    // --- USN change reasons (subset we act on) ---
    internal const uint UsnReasonFileCreate = 0x00000100;
    internal const uint UsnReasonFileDelete = 0x00000200;
    internal const uint UsnReasonRenameOldName = 0x00001000;
    internal const uint UsnReasonRenameNewName = 0x00002000;
    internal const uint UsnReasonClose = 0x80000000;

    /// <summary>The NTFS root directory always has file reference number 5.</summary>
    internal const ulong RootFileReferenceNumber = 0x0005000000000005UL;

    // Error codes worth distinguishing from generic failure.
    internal const int ErrorHandleEof = 38;
    internal const int ErrorJournalNotActive = 1179;
    internal const int ErrorJournalDeleteInProgress = 1178;
    internal const int ErrorInvalidFunction = 1;

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern SafeFileHandle CreateFileW(
        string lpFileName,
        uint dwDesiredAccess,
        uint dwShareMode,
        IntPtr lpSecurityAttributes,
        uint dwCreationDisposition,
        uint dwFlagsAndAttributes,
        IntPtr hTemplateFile);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool DeviceIoControl(
        SafeFileHandle hDevice,
        uint dwIoControlCode,
        IntPtr lpInBuffer,
        int nInBufferSize,
        IntPtr lpOutBuffer,
        int nOutBufferSize,
        out int lpBytesReturned,
        IntPtr lpOverlapped);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern uint GetLogicalDrives();

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern uint GetDriveTypeW(string lpRootPathName);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool GetVolumeInformationW(
        string lpRootPathName,
        IntPtr lpVolumeNameBuffer,
        int nVolumeNameSize,
        out uint lpVolumeSerialNumber,
        out uint lpMaximumComponentLength,
        out uint lpFileSystemFlags,
        [MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder lpFileSystemNameBuffer,
        int nFileSystemNameSize);

    [StructLayout(LayoutKind.Sequential)]
    internal struct UsnJournalDataV0
    {
        public ulong UsnJournalID;
        public long FirstUsn;
        public long NextUsn;
        public long LowestValidUsn;
        public long MaxUsn;
        public ulong MaximumSize;
        public ulong AllocationDelta;
    }

    /// <summary>Input to FSCTL_ENUM_USN_DATA. The output buffer starts with the next
    /// <see cref="StartFileReferenceNumber"/> to resume from, then packed USN records.</summary>
    [StructLayout(LayoutKind.Sequential)]
    internal struct MftEnumDataV0
    {
        public ulong StartFileReferenceNumber;
        public long LowUsn;
        public long HighUsn;
    }

    /// <summary>Input to FSCTL_READ_USN_JOURNAL. The output buffer starts with the next
    /// USN to resume from, then packed USN records.</summary>
    [StructLayout(LayoutKind.Sequential)]
    internal struct ReadUsnJournalDataV0
    {
        public long StartUsn;
        public uint ReasonMask;
        public uint ReturnOnlyOnClose;
        public ulong Timeout;
        public ulong BytesToWaitFor;
        public ulong UsnJournalID;
    }

    /// <summary>Input to FSCTL_CREATE_USN_JOURNAL.</summary>
    [StructLayout(LayoutKind.Sequential)]
    internal struct CreateUsnJournalData
    {
        public ulong MaximumSize;
        public ulong AllocationDelta;
    }
}
