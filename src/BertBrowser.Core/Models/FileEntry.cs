namespace BertBrowser.Core.Models;

/// <summary>
/// One entry in a directory listing.
/// </summary>
/// <param name="CreatedUtc">
/// When it was created, and <paramref name="AccessedUtc"/> when it was last read.
/// <see cref="System.IO.Enumeration.FileSystemEntry"/> hands both back from the same find data the
/// rest of this record is built from, so the Created and Accessed columns cost no extra I/O at all.
/// <para>
/// They default to <c>default</c>, which means <b>unknown</b> and renders blank — never
/// <c>01/01/0001</c>, which looks like data. That is what an archive entry and a search hit have:
/// neither a container's directory nor the MFT index carries these two.
/// </para>
/// </param>
public sealed record FileEntry(
    string Name,
    string FullPath,
    bool IsDirectory,
    long SizeBytes,
    DateTime ModifiedUtc,
    FileAttributes Attributes,
    DateTime CreatedUtc = default,
    DateTime AccessedUtc = default);
