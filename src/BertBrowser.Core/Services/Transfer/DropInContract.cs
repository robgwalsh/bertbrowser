namespace BertBrowser.Core.Services.Transfer;

/// <summary>Where the payload of a drop came from.</summary>
public enum DropOrigin
{
    /// <summary>A drag that started in this app, carrying the private item format.</summary>
    InApp,

    /// <summary>An ordinary <c>CF_HDROP</c> from another application.</summary>
    External,
}

/// <param name="Verb">What to do with the items.</param>
/// <param name="Report">
/// What to put in <c>DragEventArgs.Effects</c> once the drop has been handled — which is how the
/// <em>source</em> application learns what happened.
/// </param>
public readonly record struct DropInDecision(TransferVerb Verb, DropEffect Report);

/// <summary>
/// What an incoming drop means, which differs sharply depending on who started it.
/// </summary>
/// <remarks>
/// <para>
/// This is <see cref="DragOutContract"/>'s counterpart, and the danger runs the other way. There,
/// an external target's answer could make us delete our own files; here, the effect we report back
/// can make an <em>external source</em> delete its files — so reporting Move for a drag the user
/// meant as a copy destroys someone else's data on our say-so.
/// </para>
/// <para>
/// Hence the asymmetry in the defaults. An in-app drag defaults to <b>Move</b>, because both ends
/// are this app and moving between two panes is the obvious meaning. An external drag defaults to
/// <b>Copy</b>, because the source is a window we know nothing about and the safe reading of
/// "put this here" is to leave the original where it is. Shift asks for a move explicitly.
/// </para>
/// <para>
/// The other half is what to report. An in-app drop must report <see cref="DropEffect.None"/>: the
/// transfer has already happened through <c>TransferExecutor</c>, and telling our own drag source
/// "Move" would have <see cref="DragOutContract"/> read it as a foreign move and delete the items
/// we just placed. An external drop must report the verb it actually performed, or a move leaves
/// the source's copy behind.
/// </para>
/// </remarks>
public static class DropInContract
{
    private const DropEffect Verbs = DropEffect.Copy | DropEffect.Move | DropEffect.Link;

    /// <param name="origin">Which format the payload arrived in.</param>
    /// <param name="control">Whether Ctrl is down.</param>
    /// <param name="shift">Whether Shift is down.</param>
    /// <param name="allowed">
    /// What the source is willing to permit (<c>DragEventArgs.AllowedEffects</c>). A source that
    /// only offers Copy must never be told Move — it may act on that.
    /// </param>
    public static DropInDecision Decide(DropOrigin origin, bool control, bool shift, DropEffect allowed)
    {
        var verb = origin == DropOrigin.InApp
            // In-app: Ctrl copies, and everything else moves. Shift means nothing here — it is the
            // list's range-extend modifier, and has never selected the verb on this side.
            ? (control ? TransferVerb.Copy : TransferVerb.Move)
            // External: copying is the default, and a move has to be asked for. Ctrl still forces a
            // copy, so holding it can never turn into a move by accident.
            : (shift && !control ? TransferVerb.Move : TransferVerb.Copy);

        var offered = allowed & Verbs;

        // A source that will not permit the verb we chose gets the other one rather than a refusal,
        // so a drag from an app offering copy-only still works instead of silently doing nothing.
        if (verb == TransferVerb.Move && (offered & DropEffect.Move) == 0)
            verb = TransferVerb.Copy;
        else if (verb == TransferVerb.Copy && (offered & DropEffect.Copy) == 0 &&
                 (offered & DropEffect.Move) != 0)
            verb = TransferVerb.Move;

        // Our own drop reports nothing, whatever it did — see the remarks.
        var report = origin == DropOrigin.InApp
            ? DropEffect.None
            : verb == TransferVerb.Copy ? DropEffect.Copy : DropEffect.Move;

        return new DropInDecision(verb, report);
    }

    /// <summary>
    /// Whether a drop offering these effects can be accepted at all. A source offering only
    /// <see cref="DropEffect.Link"/> is asking for a shortcut, which this app does not make.
    /// </summary>
    public static bool CanAccept(DropEffect allowed) =>
        (allowed & (DropEffect.Copy | DropEffect.Move)) != 0;
}
