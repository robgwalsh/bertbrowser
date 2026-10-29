namespace BertBrowser.Core.Services.Changes;

/// <summary>
/// What one journal record amounted to, once <see cref="ChangeLogRules.Classify"/> has read its
/// reason bits. Stored as its integer in <c>fs_change.kind</c>, so the values are fixed.
/// </summary>
public enum ChangeKind
{
    Created = 1,
    Modified = 2,
    Deleted = 3,
    Renamed = 4,
}
