using System.Windows.Media;

namespace BertBrowser.App.ViewModels;

/// <summary>A top-level row in the Drives tree — either a browsable directory
/// (<see cref="DirectoryNodeViewModel"/>) or a portable device
/// (<see cref="PortableDeviceNodeViewModel"/>). Shared surface so both can live in the
/// same <c>TreeView</c> and reuse the one item template / container style.</summary>
public interface ISidebarNode
{
    string Name { get; }
    ImageSource? Icon { get; }
    string FullPath { get; }
    int Depth { get; }
    bool IsExpanded { get; set; }
    bool IsSelected { get; set; }
}
