namespace BertBrowser.Core.Services.Search;

/// <summary>
/// What a node made of one candidate: it matched, it did not, or it cannot be answered until the
/// file has been read.
/// </summary>
/// <remarks>
/// <para><strong>Three-valued because the query is asked twice.</strong> A <c>content:</c> term is
/// a question about bytes, and the first pass — an index row, or a directory entry from a walk —
/// has none. Answering "no" there would drop every real hit before the reader ever ran; answering
/// "yes" would be a superset, but a <em>lossy</em> one, because the caller could then no longer
/// tell a settled hit from a file it still has to open.</para>
/// <para>That distinction is the whole optimisation. In <c>content:a OR ext:txt</c> every
/// <c>.txt</c> is already a hit and must never be read, and in <c>is:dir content:x</c> every
/// directory is already refused and must never reach a file opener. Kleene logic gets both for
/// free; a boolean superset gets neither.</para>
/// </remarks>
public enum SearchMatch
{
    /// <summary>Settled: this candidate is not a hit, whatever its contents turn out to be.</summary>
    No,

    /// <summary>Settled: this candidate is a hit, without reading it.</summary>
    Yes,

    /// <summary>Undecided until the file is read. Only <see cref="ContentTerm"/> originates this.</summary>
    NeedsContent,
}
