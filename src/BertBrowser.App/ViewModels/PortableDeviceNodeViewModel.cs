using System.Collections.ObjectModel;
using System.Windows.Media;
using CommunityToolkit.Mvvm.ComponentModel;
using BertBrowser.App.Interop;

namespace BertBrowser.App.ViewModels;

/// <summary>A phone/camera/media player under "This PC". A leaf in the Drives tree
/// (no in-app children — its contents aren't a filesystem path); double-clicking opens
/// it in Explorer.</summary>
public sealed partial class PortableDeviceNodeViewModel : ObservableObject, ISidebarNode
{
    private readonly FolderTreeViewModel _tree;

    public PortableDevice Device { get; }

    public string Name => Device.Name;
    public string FullPath => Device.ShellPath;
    public int Depth => 0;

    // No filesystem path to resolve a real icon from; a generic device glyph reads clearly.
    public ImageSource? Icon { get; } = ShellIcons.GetComputerIcon();

    /// <summary>Present so the shared tree template's <c>{Binding Children}</c> resolves;
    /// always empty, so no expander shows.</summary>
    public ObservableCollection<ISidebarNode> Children { get; } = new();

    [ObservableProperty]
    private bool _isExpanded;

    [ObservableProperty]
    private bool _isSelected;

    partial void OnIsSelectedChanged(bool value)
    {
        // A device is a leaf (nothing to expand), but selecting it still makes it the active
        // accordion item, so collapse any open drive above it.
        if (value) _tree.CollapseOtherRoots(this);
    }

    public PortableDeviceNodeViewModel(FolderTreeViewModel tree, PortableDevice device)
    {
        _tree = tree;
        Device = device;
    }
}
