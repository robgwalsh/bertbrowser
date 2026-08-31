using BertBrowser.Core.Cli;

namespace BertBrowser.Core.Ipc;

/// <summary>What the two ends of the elevated-operation pipe can say to each other.</summary>
public enum ElevationVerb
{
    /// <summary>Both ways, first: the protocol version. A mismatch ends the session.</summary>
    Hello,

    /// <summary>Helper → app: connected, checked, and waiting for a request.</summary>
    Ready,

    /// <summary>App → helper: the operation's header. Exactly one per session.</summary>
    Begin,

    /// <summary>App → helper: one item of the request. Never more than
    /// <see cref="ElevationProtocol.MaxItems"/> of them.</summary>
    Item,

    /// <summary>App → helper: the request is complete; carry it out.</summary>
    Go,

    /// <summary>App → helper: stop. The executors' own cancel guarantees apply — nothing is left
    /// half-written and whatever got across stays across.</summary>
    Cancel,

    /// <summary>Helper → app: how far along, coalesced to the usual 100 ms.</summary>
    Progress,

    /// <summary>Helper → app: one item succeeded.</summary>
    Done,

    /// <summary>Helper → app: one item failed.</summary>
    Fault,

    /// <summary>Helper → app: the operation is over, with whatever the outcome needs that is not
    /// per-item — staging folders, the cancelled flag.</summary>
    End,

    /// <summary>Helper → app: the session cannot continue. Always the last thing it says.</summary>
    Fatal,
}

/// <summary>One line of the elevated-operation protocol.</summary>
public readonly record struct ElevationMessage(ElevationVerb Verb, string Payload = "")
{
    public override string ToString() => ElevationProtocol.Format(this);
}

/// <summary>
/// The line format for the elevated file-operation helper: <c>OPS\t&lt;Verb&gt;</c>, optionally
/// followed by a tab and one JSON document.
/// </summary>
/// <remarks>
/// <para>
/// <b>Unlike the index protocol, this one carries paths, and it has to.</b> The index helper's rule
/// — four verbs and never a path — is a rule about <em>that</em> helper, and rests on three
/// properties of it: it lives for the whole session, it is started at launch without anyone asking,
/// and its job (reading a volume) names no file. A path verb there would let anything reaching the
/// pipe aim an always-on administrator-token process at a chosen file with no user gesture in
/// between.
/// </para>
/// <para>
/// This helper inverts all three. It lives for one operation and exits, it is started only by a
/// click on a shield in a dialog that names the items, and it exists <em>because</em> it takes
/// paths — an elevated file operation naming no file would do nothing. So what replaces "never a
/// path" here is <b>one prompt per operation, one request per process, and the process exits when
/// the request is done</b>, and that is made structural rather than left to convention:
/// <c>ElevationHost</c> accepts <see cref="ElevationVerb.Item"/> only before
/// <see cref="ElevationVerb.Go"/>, and after <c>Go</c> the only verb it will read is
/// <see cref="ElevationVerb.Cancel"/>.
/// </para>
/// <para>
/// <b>The bound is two-dimensional, and neither half should ever need raising.</b> No line is more
/// than one record, so the line cap stays <see cref="NavigationRequest.MaxLineLength"/> — the same
/// bound <c>LineChannel</c> exists to enforce. The number of records is capped separately by
/// <see cref="MaxItems"/>, refused as the host reads rather than after it has grown a buffer. A cap
/// that has to be raised to fit a big operation is not a cap; sending the whole plan on one line
/// would have needed exactly that.
/// </para>
/// <para>
/// Tab is safe as the separator for the reason it is in the index protocol: every path is checked
/// against <see cref="NavigationRequest.IsAcceptablePath"/>, which rejects control characters, and
/// <c>System.Text.Json</c> escapes any that survive inside a string. A single-line JSON document
/// contains no raw tab or newline.
/// </para>
/// </remarks>
public static class ElevationProtocol
{
    /// <summary>Bumped when the shape of anything on the wire changes. Both ends greet with it and a
    /// mismatch ends the session rather than risking a misread plan.</summary>
    public const int ProtocolVersion = 1;

    /// <summary>Comfortably past any selection a person makes, and small enough that a hostile peer
    /// cannot make the helper accumulate. A request over this is refused whole.</summary>
    public const int MaxItems = 10_000;

    /// <summary>The status text on <see cref="ElevationVerb.Fatal"/>.</summary>
    public const int MaxStatusLength = 200;

    private const char Separator = '\t';
    private const string Prefix = "OPS";

    public static string Format(ElevationMessage message) =>
        message.Payload.Length == 0
            ? $"{Prefix}{Separator}{message.Verb}"
            : $"{Prefix}{Separator}{message.Verb}{Separator}{message.Payload}";

    /// <summary>Parses a line, or refuses it. Never throws: everything on this wire is untrusted.</summary>
    public static bool TryParse(string? line, out ElevationMessage message)
    {
        message = default;
        if (string.IsNullOrEmpty(line) || line.Length > NavigationRequest.MaxLineLength) return false;

        // Two fields, or three where the third is the payload — which may itself contain nothing
        // that looks like a separator, since JSON escapes control characters.
        var parts = line.Split(Separator, 3);
        if (parts.Length is < 2 or > 3) return false;
        if (!string.Equals(parts[0], Prefix, StringComparison.Ordinal)) return false;

        if (!Enum.TryParse<ElevationVerb>(parts[1], out var verb)) return false;
        // Enum.TryParse accepts numbers and comma-separated composites; neither is a verb.
        if (!Enum.IsDefined(verb) || !string.Equals(parts[1], verb.ToString(), StringComparison.Ordinal))
            return false;

        var payload = parts.Length == 3 ? parts[2] : "";
        if (!IsAcceptablePayload(verb, payload)) return false;

        message = new ElevationMessage(verb, payload);
        return true;
    }

    /// <summary>Whether a verb may carry this payload at all — the shape check, before anything
    /// tries to read the JSON inside it.</summary>
    public static bool IsAcceptablePayload(ElevationVerb verb, string payload) => verb switch
    {
        ElevationVerb.Hello => IsAcceptableVersion(payload),
        ElevationVerb.Fatal => IsAcceptableStatus(payload),
        ElevationVerb.Ready or ElevationVerb.Go or ElevationVerb.Cancel => payload.Length == 0,
        // Begin, Item, Progress, Done, Fault, End: one JSON document, non-empty.
        _ => payload.Length > 0,
    };

    /// <summary>The version on a greeting, or null when it is not one.</summary>
    public static int? VersionOf(ElevationMessage message) =>
        message.Verb == ElevationVerb.Hello && int.TryParse(message.Payload, out var version)
            ? version
            : null;

    private static bool IsAcceptableVersion(string payload) =>
        int.TryParse(payload, out var version) && version > 0;

    private static bool IsAcceptableStatus(string payload) =>
        payload.Length is > 0 and <= MaxStatusLength && !payload.Any(char.IsControl);

    /// <summary>Flattens an exception into something safe to put on a <see cref="ElevationVerb.Fatal"/>
    /// line: one line, no control characters, bounded.</summary>
    public static string Summarize(string text)
    {
        var flattened = new string([.. text.Select(c => char.IsControl(c) ? ' ' : c)]).Trim();
        if (flattened.Length == 0) return "The elevated helper failed.";
        return flattened.Length <= MaxStatusLength ? flattened : flattened[..MaxStatusLength];
    }
}
