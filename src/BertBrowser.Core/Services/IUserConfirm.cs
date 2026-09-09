namespace BertBrowser.Core.Services;

/// <summary>
/// Asking the user a yes/no question about something they just asked for.
/// </summary>
/// <remarks>
/// <para>
/// The half <see cref="IUserNotice"/> does not do. That one tells; this one asks, and the difference
/// is not tidiness — an answer changes what happens next, so it cannot be recorded and dropped the
/// way a message can. A seam rather than a call into a dialog for the reason
/// <see cref="Elevation.IElevationPrompt"/> is one: a modal raised from inside the shell would block
/// the scripted run that hosts the same window offscreen, and a test has nobody to ask at all.
/// </para>
/// <para>
/// For a gesture whose cost the user cannot see before making it — flattening a drive root is the
/// first. Not for confirming anything destructive: those have dialogs of their own that show what
/// is about to happen.
/// </para>
/// </remarks>
public interface IUserConfirm
{
    /// <summary>True to go ahead.</summary>
    /// <param name="confirmLabel">What the button that goes ahead should say. A named action reads
    /// as a choice; "OK" reads as an obstacle.</param>
    bool Ask(string message, string caption, string confirmLabel);
}

/// <summary>Answers no, always. What a context with nobody to ask gets.</summary>
/// <remarks>
/// No rather than yes, on the same principle <c>RefusingElevationPrompt</c> takes: an unanswered
/// question should leave things as they were, and the only cost of declining here is that a toggle
/// does nothing.
/// </remarks>
public sealed class DecliningUserConfirm : IUserConfirm
{
    public bool Ask(string message, string caption, string confirmLabel) => false;
}
