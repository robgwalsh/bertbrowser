using System.Collections.Concurrent;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Archives;

namespace BertBrowser.App.Services;

/// <summary>
/// Passwords the user has given for encrypted archives, for as long as this process runs.
/// </summary>
/// <remarks>
/// <para>
/// <b>Nothing is persisted, and the reason is not vagueness about cryptography.</b>
/// <c>settings.json</c> is a plain file in the profile and <c>bertbrowser.db</c> is a plain SQLite
/// file beside it, so "remembered" would mean "written in the clear next to the archive it
/// unlocks". DPAPI would fix the file and not the design: a file browser that silently remembers
/// archive passwords is a credential store, and this app is not one. Closing the window forgets
/// them, and that is the whole contract.
/// </para>
/// <para>
/// <b>Keyed by path plus length plus write time</b>, the same key <see cref="ArchiveCache"/> uses.
/// A rewritten archive is a different archive, and being asked again is the correct answer rather
/// than an inconvenience.
/// </para>
/// </remarks>
public sealed class ArchivePasswordStore : IArchivePasswords
{
    private readonly ConcurrentDictionary<string, string> _passwords = new(StringComparer.Ordinal);

    public string? For(string archiveFile) =>
        KeyFor(archiveFile) is { } key && _passwords.TryGetValue(key, out var password)
            ? password
            : null;

    /// <summary>Remembers a password for this session only.</summary>
    public void Remember(string archiveFile, string password)
    {
        if (KeyFor(archiveFile) is { } key) _passwords[key] = password;
    }

    /// <summary>Forgets one archive's password — what a refused password does.</summary>
    public void Forget(string archiveFile)
    {
        if (KeyFor(archiveFile) is { } key) _passwords.TryRemove(key, out _);
    }

    /// <summary>Forgets everything. Called when the window closes.</summary>
    public void Clear() => _passwords.Clear();

    private static string? KeyFor(string archiveFile)
    {
        try
        {
            var info = new FileInfo(archiveFile);
            if (!info.Exists) return null;
            return $"{PathKey.Canonicalize(archiveFile)}|{info.Length}|{info.LastWriteTimeUtc.Ticks}";
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                                       or ArgumentException or NotSupportedException)
        {
            return null;
        }
    }
}
