namespace BertBrowser.Core.Services.Search;

/// <summary>
/// The decoded text of one file, held just long enough for the query to be asked about it.
/// </summary>
/// <remarks>
/// <para><strong>Never retain one of these.</strong> The scanner builds a snippet from it and drops
/// it inside the worker: at the candidate ceiling, keeping them would mean tens of thousands of
/// megabyte strings alive until the search ended. It lives on <see cref="SearchCandidate"/>, which
/// is a stack value passed by <c>in</c>, precisely so it has nowhere to accumulate.</para>
/// <para><see cref="None"/> is the answer for something with no text to search — a directory, a
/// file that would not open, a binary. It is not the same as a null
/// <see cref="SearchCandidate.Content"/>, which means "not read yet": <see cref="None"/> settles a
/// content term as <see cref="SearchMatch.No"/>, where null leaves it
/// <see cref="SearchMatch.NeedsContent"/>.</para>
/// </remarks>
public sealed class ContentText
{
    /// <summary>Nothing to search: a directory, an unreadable file, or one that is not text.</summary>
    public static readonly ContentText None = new("", truncated: false);

    public ContentText(string text, bool truncated)
    {
        Text = text;
        Truncated = truncated;
    }

    /// <summary>The decoded text, line endings normalised to <c>\n</c>.</summary>
    public string Text { get; }

    /// <summary>
    /// The file was longer than the per-file budget and this is only its front.
    /// </summary>
    /// <remarks>
    /// A miss against a truncated read is "not in the first megabyte", not "not in the file", and
    /// the outcome says so rather than quietly under-reporting — the same honesty
    /// <c>DeleteSurveyor</c> keeps with <c>Incomplete</c>.
    /// </remarks>
    public bool Truncated { get; }

    /// <summary>
    /// Where <paramref name="needle"/> first appears, or -1.
    /// </summary>
    /// <remarks>
    /// <c>OrdinalIgnoreCase</c> rather than the uppercase-the-candidate trick <see cref="NameTerm"/>
    /// uses, and the difference is deliberate: the index stores names already folded, so that term
    /// gets its folding for free, while folding a megabyte of file text would allocate a second
    /// megabyte per file across four threads. Measured, this comparison costs the same as an ordinal
    /// one — both are vectorised — so the fold would buy nothing at all. It also hands back the
    /// offset the snippet needs.
    /// </remarks>
    public int IndexOf(string needle) =>
        Text.IndexOf(needle, StringComparison.OrdinalIgnoreCase);
}
