namespace BertBrowser.Core.Services.Changes;

/// <summary>How far back the timeline looks. <see cref="SinceMark"/> is the installer case: press
/// "Mark now", run the thing, and see only what happened after.</summary>
public enum ChangeRange
{
    Last15Minutes,
    LastHour,
    Last6Hours,
    Last24Hours,
    SinceMark,
}
