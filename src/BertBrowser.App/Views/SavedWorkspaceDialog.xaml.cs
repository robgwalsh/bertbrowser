using System.Windows;
using System.Windows.Controls;
using BertBrowser.Core.Services.SavedWorkspaces;

namespace BertBrowser.App.Views;

/// <summary>
/// Names a workspace — for a new save or a rename of an existing one.
/// </summary>
/// <remarks>
/// Re-validates through <see cref="SavedWorkspaceRules.Validate"/> on every change. A <em>new</em>
/// workspace saved under a name already in use replaces it, and the button says so; a
/// <em>rename</em> may not take another workspace's name, since that would swallow it. Used for
/// both entry points — "save current layout" and "rename existing row" — since the only field is
/// the name either way.
/// </remarks>
public partial class SavedWorkspaceDialog : ThemedWindow
{
    private readonly Func<string, bool> _nameTaken;
    private readonly string? _editingName;
    private string? _result;
    private bool _ready;

    private SavedWorkspaceDialog(string seedName, Func<string, bool> nameTaken, string? editingName)
    {
        InitializeComponent();
        _nameTaken = nameTaken;
        _editingName = editingName;

        Title = editingName is null ? "Save workspace" : "Rename workspace";
        NameBox.Text = seedName;

        _ready = true;
        Revalidate();
        Loaded += (_, _) =>
        {
            NameBox.Focus();
            NameBox.SelectAll();
        };
    }

    /// <summary>The dialog built but not shown, for the UI harness to park offscreen and
    /// photograph. Nothing in the app uses it: <see cref="Show"/> is the only way in from the
    /// interface, and it goes through this same constructor.</summary>
    internal static SavedWorkspaceDialog Create(string seedName, Func<string, bool> nameTaken, string? editingName = null) =>
        new(seedName, nameTaken, editingName);

    /// <summary>Shows the dialog and returns the name to save under, or null if it was
    /// cancelled.</summary>
    /// <param name="nameTaken">Whether a name belongs to another saved workspace — already
    /// excluding <paramref name="editingName"/>, the one being edited.</param>
    public static string? Show(Window? owner, string seedName, Func<string, bool> nameTaken, string? editingName = null)
    {
        var dialog = new SavedWorkspaceDialog(seedName, nameTaken, editingName);
        if (owner is not null && !ReferenceEquals(owner, dialog)) dialog.Owner = owner;

        return dialog.ShowDialog() == true ? dialog._result : null;
    }

    private void Field_TextChanged(object sender, TextChangedEventArgs e) => Revalidate();

    private void Revalidate()
    {
        if (!_ready) return;

        var name = NameBox.Text.Trim();

        // A new workspace may take an existing name (it replaces); a rename may not.
        var replacing = _editingName is null && name.Length > 0 && _nameTaken(name);
        var problem = SavedWorkspaceRules.Validate(name, _editingName is null ? _ => false : _nameTaken);

        ProblemText.Text = problem ?? "";
        ProblemText.Visibility = problem is null ? Visibility.Collapsed : Visibility.Visible;
        OkButton.Content = replacing ? "Replace" : (_editingName is null ? "Save" : "Rename");
        OkButton.IsEnabled = problem is null;

        _result = problem is null ? name : null;
    }

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        Revalidate();
        if (!OkButton.IsEnabled) return;
        DialogResult = true;
    }
}
