namespace BertStat.Core.Models;

public sealed record FileEntry(
    string Name,
    string FullPath,
    bool IsDirectory,
    long SizeBytes,
    DateTime ModifiedUtc,
    FileAttributes Attributes);
