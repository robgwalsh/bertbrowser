using BertBrowser.Core.Cli;

namespace BertBrowser.Core.Ipc;

/// <summary>What one message on the index pipe means.</summary>
public enum IndexVerb
{
    /// <summary>Both ways, first: the protocol version each end speaks.</summary>
    Hello,

    /// <summary>Helper → app: the database is open and volumes are enumerated.</summary>
    Ready,

    /// <summary>App → helper: begin indexing.</summary>
    Start,

    /// <summary>App → helper: stop and exit.</summary>
    Shutdown,

    /// <summary>Helper → app: this drive letter's initial enumeration has begun.</summary>
    Building,

    /// <summary>Helper → app: this root key's index is complete.</summary>
    Complete,

    /// <summary>Helper → app: this drive letter is no longer building, complete or not.</summary>
    Idle,

    /// <summary>Helper → app: indexing cannot continue, and why.</summary>
    Fatal,

    /// <summary>Either way: are you there?</summary>
    Ping,

    /// <summary>The answer to <see cref="Ping"/>.</summary>
    Pong,
}

/// <summary>One message: a verb and at most one argument.</summary>
public readonly record struct IndexMessage(IndexVerb Verb, string Argument = "")
{
    public override string ToString() => IndexProtocol.Format(this);
}

/// <summary>
/// The wire format between BertBrowser and the elevated process that reads the MFT for it.
/// </summary>
/// <remarks>
/// <para>
/// <b>The elevated end accepts four verbs and never a path.</b> <see cref="IndexVerb.Hello"/>,
/// <see cref="IndexVerb.Start"/>, <see cref="IndexVerb.Shutdown"/> and <see cref="IndexVerb.Ping"/>
/// are the whole of what a medium-integrity process can ask an administrator-token process to do,
/// and none of them names a file. That is the property worth protecting: a later "re-index this
/// folder" verb would put an attacker-chosen path on the elevated surface, and there is no version
/// of that which is worth the convenience.
/// </para>
/// <para>
/// <b>The rule survives the arrival of a second elevated helper, and it is worth being clear why.</b>
/// It was never "no elevated process may take a path". It is a rule about <em>this</em> helper, and
/// it rests on three properties of it: it lives for the whole session, it is started at launch
/// without anyone asking, and its job — reading a volume — names no file. A path verb here would let
/// anything reaching this pipe aim an always-on administrator-token process at a chosen file, with
/// no user gesture in between. <c>BertBrowser.Elevator</c> inverts all three: it lives for one
/// operation, is started only by a click on a shield in a dialog naming the items, and exists
/// <em>because</em> it takes paths. What replaces the rule there is one prompt per operation, one
/// request per process, and a process that exits when the request is done.
/// </para>
/// <para>
/// Everything in the other direction is a state push, not a reply. The app mirrors what arrives
/// into an <c>MftIndexState</c> and answers <c>IsIndexed</c>/<c>AnyIndexed</c>/<c>StatusText</c>
/// from it locally, so nothing the UI asks ever waits on a round trip.
/// </para>
/// <para>
/// Format and discipline are <see cref="NavigationRequest"/>'s, deliberately: one line per message,
/// tab-separated, capped at <see cref="NavigationRequest.MaxLineLength"/>. Tab is safe as a
/// separator precisely because every argument rule below rejects control characters. A line that
/// fails to parse is <em>ignored and reading continues</em> — one malformed message must not end a
/// session, the same lesson the single-instance listener already carries.
/// </para>
/// </remarks>
public static class IndexProtocol
{
    /// <summary>
    /// Bumped whenever the meaning of a verb changes. Both ends ship in the same package and are
    /// launched by path, so a mismatch should be impossible — but a half-applied update leaves a
    /// stale executable behind, and refusing to talk to it is much better than mirroring state from
    /// something that means something else by it.
    /// </summary>
    public const int ProtocolVersion = 1;

    /// <summary>A status line long enough to be useful and short enough not to be a payload.</summary>
    public const int MaxStatusLength = 200;

    private const char Separator = '\t';
    private const string Prefix = "IDX";

    /// <summary>Renders a message as one line.</summary>
    public static string Format(IndexMessage message) =>
        string.IsNullOrEmpty(message.Argument)
            ? $"{Prefix}{Separator}{message.Verb}"
            : $"{Prefix}{Separator}{message.Verb}{Separator}{message.Argument}";

    /// <summary>
    /// Reads a line back. False for a wrong prefix, an unknown verb, an oversized line, or an
    /// argument the verb's own rule refuses — callers ignore the line and keep reading.
    /// </summary>
    public static bool TryParse(string? line, out IndexMessage message)
    {
        message = default;
        if (string.IsNullOrEmpty(line) || line.Length > NavigationRequest.MaxLineLength) return false;

        var parts = line.Split(Separator);
        if (parts.Length is < 2 or > 3) return false;
        if (!parts[0].Equals(Prefix, StringComparison.Ordinal)) return false;
        if (!Enum.TryParse<IndexVerb>(parts[1], ignoreCase: false, out var verb)) return false;
        // Enum.TryParse accepts the underlying number ("7") and comma-separated lists as well as
        // names, neither of which is a verb anyone meant to send.
        if (!Enum.IsDefined(verb) || !parts[1].Equals(verb.ToString(), StringComparison.Ordinal))
            return false;

        var argument = parts.Length == 3 ? parts[2] : "";
        if (!IsAcceptableArgument(verb, argument)) return false;

        message = new IndexMessage(verb, argument);
        return true;
    }

    /// <summary>Each verb's own rule for what it may carry.</summary>
    public static bool IsAcceptableArgument(IndexVerb verb, string argument) => verb switch
    {
        IndexVerb.Hello => IsAcceptableVersion(argument),
        IndexVerb.Building or IndexVerb.Idle => IsAcceptableDrive(argument),
        IndexVerb.Complete => IsAcceptableRootKey(argument),
        IndexVerb.Fatal => IsAcceptableStatus(argument),
        // Ready, Start, Shutdown, Ping and Pong say everything by arriving.
        _ => argument.Length == 0,
    };

    /// <summary>A bare drive letter — "C", not "C:" and not a path.</summary>
    public static bool IsAcceptableDrive(string? candidate) =>
        candidate is { Length: 1 } && candidate[0] is >= 'A' and <= 'Z';

    /// <summary>
    /// A canonical volume root, as <c>PathKey.Canonicalize</c> produces: "C:\" and nothing else.
    /// </summary>
    /// <remarks>
    /// Narrower than <see cref="NavigationRequest.IsAcceptablePath"/> on purpose, and it still goes
    /// through it first. The only root the indexer ever completes is a volume, so accepting a
    /// deeper path here would only widen what a peer can put into the state the search router
    /// trusts.
    /// </remarks>
    public static bool IsAcceptableRootKey(string? candidate)
    {
        if (!NavigationRequest.IsAcceptablePath(candidate)) return false;
        return candidate is { Length: 3 } &&
               candidate[0] is >= 'A' and <= 'Z' &&
               candidate[1] == ':' &&
               candidate[2] == '\\';
    }

    /// <summary>
    /// Text bound for the status bar: bounded, and no control characters. Only
    /// <see cref="IndexVerb.Fatal"/> carries any — the wording of the ordinary indexing line is
    /// derived on the app's side from the drives it was told about, so there is one place that
    /// decides it and no way for the two processes to disagree.
    /// </summary>
    public static bool IsAcceptableStatus(string? candidate)
    {
        if (candidate is null) return false;
        if (candidate.Length is 0 or > MaxStatusLength) return false;

        foreach (var c in candidate)
        {
            if (char.IsControl(c)) return false;
        }
        return true;
    }

    private static bool IsAcceptableVersion(string? candidate) =>
        int.TryParse(candidate, out var version) && version > 0;

    /// <summary>The version an end is announcing, or null if it is not a <see cref="IndexVerb.Hello"/>.</summary>
    public static int? VersionOf(IndexMessage message) =>
        message.Verb == IndexVerb.Hello && int.TryParse(message.Argument, out var version)
            ? version
            : null;
}
