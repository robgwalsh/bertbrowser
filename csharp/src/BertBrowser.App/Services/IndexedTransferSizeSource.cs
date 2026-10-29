using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.App.Services;

/// <summary>
/// Answers "how big is this?" for a transfer out of <c>dir_size_cache</c>.
/// </summary>
/// <remarks>
/// <para>
/// <b>Nothing here walks the filesystem.</b> The MFT pass has already written a row for every
/// directory on the volume, so a folder's recursive size is one batched <see cref="DirSizeRepository.GetMany"/>
/// over the plan — the same call the file list and the folder tree make. Sizing a 200,000-file tree
/// by walking it before starting would cost more than the transfer.
/// </para>
/// <para>
/// <b>A missing row is unknown, never zero.</b> That is what a non-NTFS volume, or one still being
/// indexed, looks like — and <see cref="TransferEstimator"/> turns it into an estimate that is only
/// a floor, which is what suppresses the percentage and the time remaining. An
/// <see cref="DirSizeResult.Incomplete"/> row is treated the same way, because it is a floor too.
/// </para>
/// </remarks>
internal sealed class IndexedTransferSizeSource : ITransferSizeSource
{
    private readonly IReadOnlyDictionary<string, DirSizeResult> _sizes;

    private IndexedTransferSizeSource(IReadOnlyDictionary<string, DirSizeResult> sizes) => _sizes = sizes;

    /// <summary>One query for every directory the plan touches.</summary>
    internal static ITransferSizeSource For(TransferPlan plan, DirSizeRepository repository)
    {
        var directories = plan.Transfers
            .Where(t => t.IsDirectory && TransferEstimator.MovesBytes(plan.Verb, t))
            .Select(t => t.SourcePath)
            .ToList();

        return new IndexedTransferSizeSource(
            directories.Count == 0
                ? new Dictionary<string, DirSizeResult>(StringComparer.Ordinal)
                : repository.GetMany(directories));
    }

    public DirectorySize? Directory(string path) =>
        _sizes.TryGetValue(PathKey.Canonicalize(path), out var row) && !row.Incomplete
            ? new DirectorySize(row.SizeBytes, row.FileCount)
            : null;

    public long? File(string path)
    {
        try
        {
            var info = new FileInfo(path);
            return info.Exists ? info.Length : null;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
        {
            return null;
        }
    }
}
