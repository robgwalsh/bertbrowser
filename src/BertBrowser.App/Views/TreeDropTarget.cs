using System.Windows;
using System.Windows.Controls;
using BertBrowser.App.ViewModels;

namespace BertBrowser.App.Views;

/// <summary>
/// Dropping onto a folder-tree node. Attached exactly once, by the window, because the tree is
/// shared by every pane: if each pane's file-list controller also subscribed to it, one drop on a
/// tree node would be planned and carried out once per open pane.
/// </summary>
internal sealed class TreeDropTarget
{
    private readonly DropPipeline _pipeline;

    public static TreeDropTarget Attach(TreeView tree, ShellViewModel shell) => new(tree, shell);

    private TreeDropTarget(TreeView tree, ShellViewModel shell)
    {
        _pipeline = new DropPipeline(shell, shell.SetStatus);

        tree.AllowDrop = true;
        tree.DragOver += OnDragOver;
        tree.DragLeave += (_, _) => _pipeline.ClearHighlight();
        tree.Drop += OnDrop;
    }

    private void OnDragOver(object sender, DragEventArgs e) =>
        _pipeline.HandleDragOver(e, TreeNode(e)?.FullPath, TreeHighlight(e));

    private void OnDrop(object sender, DragEventArgs e) =>
        _pipeline.HandleDrop(e, TreeNode(e)?.FullPath);

    private static DirectoryNodeViewModel? TreeNode(DragEventArgs e) =>
        VisualTreeUtil.FindAncestor<TreeViewItem>(e.OriginalSource as DependencyObject)?.DataContext
            is DirectoryNodeViewModel { FullPath.Length: > 0 } node
            ? node
            : null;

    private static DependencyObject? TreeHighlight(DragEventArgs e) =>
        TreeNode(e) is not null
            ? VisualTreeUtil.FindAncestor<TreeViewItem>(e.OriginalSource as DependencyObject)
            : null;
}
