namespace BertBrowser.Core.Services.Elevation;

/// <summary>
/// An operation's result after the elevated retry has had its turn: the merged outcome, and a
/// note for the status line when the retry was declined or could not be offered.
/// </summary>
public readonly record struct Elevated<T>(T Outcome, string Note);
