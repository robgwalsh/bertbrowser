namespace BertBrowser.Core.Services.Checksums;

/// <summary>How one file came out of a verify run.</summary>
public enum ChecksumVerifyState
{
    /// <summary>Listed, present, and the digest agrees.</summary>
    Ok,

    /// <summary>Listed and present, and the bytes are not what the file says they should be.</summary>
    Mismatch,

    /// <summary>Listed, and not there.</summary>
    Missing,

    /// <summary>Listed, present, and could not be read — which is not the same as wrong.</summary>
    Unreadable,

    /// <summary>The listed name would resolve outside the folder. See <see cref="ChecksumPath"/>.</summary>
    Refused,

    /// <summary>Present in the folder and absent from the checksum file.</summary>
    NotListed,
}

public sealed record ChecksumVerifyRow(
    string Name,
    ChecksumVerifyState State,
    string? Expected,
    string? Actual);

/// <summary>
/// Reconciling what a checksum file claims against what the folder actually holds.
/// </summary>
/// <remarks>
/// Pure: the caller does the reading and hands the results in, so every rule here is testable
/// without a disk. The six states are deliberately six rather than a boolean — "could not be read"
/// is not "wrong", and "there but unlisted" is not a failure at all, it is how you find out a
/// folder gained a file.
/// </remarks>
public static class ChecksumVerify
{
    /// <summary>
    /// Builds the report.
    /// </summary>
    /// <param name="listed">The checksum file's lines, already parsed.</param>
    /// <param name="computed">
    /// A digest per listed name, or null against a name that is present but could not be read.
    /// A name absent from this dictionary was not found on disk at all.
    /// </param>
    /// <param name="refused">Listed names <see cref="ChecksumPath"/> would not resolve.</param>
    /// <param name="present">
    /// Every file actually in the folder, so the ones nothing listed can be reported. Pass an empty
    /// set to leave <see cref="ChecksumVerifyState.NotListed"/> out of the report entirely.
    /// </param>
    public static IReadOnlyList<ChecksumVerifyRow> Reconcile(
        IReadOnlyList<ChecksumLine> listed,
        IReadOnlyDictionary<string, string?> computed,
        IReadOnlyCollection<string> refused,
        IReadOnlyCollection<string> present)
    {
        var rows = new List<ChecksumVerifyRow>(listed.Count);
        var accounted = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var refusedSet = new HashSet<string>(refused, StringComparer.OrdinalIgnoreCase);

        foreach (var line in listed)
        {
            accounted.Add(line.Name);

            if (refusedSet.Contains(line.Name))
            {
                rows.Add(new ChecksumVerifyRow(line.Name, ChecksumVerifyState.Refused, line.Digest, null));
                continue;
            }

            if (!computed.TryGetValue(line.Name, out var actual))
            {
                rows.Add(new ChecksumVerifyRow(line.Name, ChecksumVerifyState.Missing, line.Digest, null));
                continue;
            }

            if (actual is null)
            {
                rows.Add(new ChecksumVerifyRow(line.Name, ChecksumVerifyState.Unreadable, line.Digest, null));
                continue;
            }

            var state = string.Equals(actual, line.Digest, StringComparison.OrdinalIgnoreCase)
                ? ChecksumVerifyState.Ok
                : ChecksumVerifyState.Mismatch;

            rows.Add(new ChecksumVerifyRow(line.Name, state, line.Digest, actual));
        }

        foreach (var name in present)
        {
            if (!accounted.Contains(name))
                rows.Add(new ChecksumVerifyRow(name, ChecksumVerifyState.NotListed, null, null));
        }

        return rows;
    }

    /// <summary>
    /// Whether a state means the verify failed.
    /// </summary>
    /// <remarks>
    /// <see cref="ChecksumVerifyState.NotListed"/> is not a failure: a folder holding a file the
    /// checksum file never mentioned is normal — the checksum file covers what it covers. Saying
    /// otherwise would make every download folder fail its own verification.
    /// </remarks>
    public static bool IsFailure(ChecksumVerifyState state) =>
        state is ChecksumVerifyState.Mismatch or ChecksumVerifyState.Missing
              or ChecksumVerifyState.Unreadable or ChecksumVerifyState.Refused;

    /// <summary>One line summarising a report, in the order that matters most first.</summary>
    public static string Summarise(IReadOnlyList<ChecksumVerifyRow> rows)
    {
        if (rows.Count == 0) return "Nothing to verify.";

        var parts = new List<string>();
        void Add(ChecksumVerifyState state, string word)
        {
            var n = rows.Count(r => r.State == state);
            if (n > 0) parts.Add($"{n:N0} {word}");
        }

        Add(ChecksumVerifyState.Mismatch, "mismatched");
        Add(ChecksumVerifyState.Missing, "missing");
        Add(ChecksumVerifyState.Unreadable, "unreadable");
        Add(ChecksumVerifyState.Refused, "refused");
        Add(ChecksumVerifyState.Ok, "OK");
        Add(ChecksumVerifyState.NotListed, "not listed");

        return string.Join(" · ", parts);
    }
}
