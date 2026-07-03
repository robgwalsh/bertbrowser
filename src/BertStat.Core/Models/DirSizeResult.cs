namespace BertStat.Core.Models;

public sealed record DirSizeResult(
    string PathKey,
    long SizeBytes,
    int FileCount,
    int DirCount,
    bool Incomplete,
    DateTime ComputedUtc);
