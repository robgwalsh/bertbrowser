using System.IO;
using System.Windows;
using System.Windows.Controls;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services.SavedSearches;

namespace BertBrowser.App.Views;

/// <summary>
/// Names a search and says where it runs — for a new saved search or an existing one.
/// </summary>
/// <remarks>
/// Re-validates through <see cref="SavedSearchRules.Validate"/> on every change, so a bad query
/// or a missing folder is refused here, in words, by the rule the save obeys. A <em>new</em>
/// search saved under a name already in use replaces it, and the button says so; an
/// <em>edit</em> may not take another search's name, since that would swallow it.
/// </remarks>
public partial class SavedSearchDialog : ThemedWindow
{
    private readonly string? _folder;
    private readonly Func<string, bool> _nameTaken;
    private readonly string? _editingName;
    private SavedSearch? _result;
    private bool _ready;

    private SavedSearchDialog(SavedSearchSeed seed, Func<string, bool> nameTaken, string? editingName)
    {
        InitializeComponent();
        _folder = seed.Folder;
        _nameTaken = nameTaken;
        _editingName = editingName;

        Title = editingName is null ? "Save search" : "Edit saved search";
        NameBox.Text = seed.Name;
        QueryBox.Text = seed.Query;

        FolderText.Text = _folder ?? "(no folder to pin)";
        ScopeFolder.IsEnabled = _folder is not null;
        var scope = seed.Scope == SavedSearchScope.Folder && _folder is null ? SavedSearchScope.CurrentFolder : seed.Scope;
        (scope switch
        {
            SavedSearchScope.Folder => ScopeFolder,
            SavedSearchScope.ThisPc => ScopePc,
            _ => ScopeCurrent,
        }).IsChecked = true;

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
    internal static SavedSearchDialog Create(SavedSearchSeed seed, Func<string, bool> nameTaken, string? editingName = null) =>
        new(seed, nameTaken, editingName);

    /// <summary>Shows the dialog and returns the search to store, or null if it was cancelled.</summary>
    /// <param name="nameTaken">Whether a name belongs to another saved search — already excluding
    /// <paramref name="editingName"/>, the one being edited.</param>
    public static SavedSearch? Show(Window? owner, SavedSearchSeed seed, Func<string, bool> nameTaken, string? editingName = null)
    {
        var dialog = new SavedSearchDialog(seed, nameTaken, editingName);
        if (owner is not null && !ReferenceEquals(owner, dialog)) dialog.Owner = owner;

        return dialog.ShowDialog() == true ? dialog._result : null;
    }

    private SavedSearchScope Scope =>
        ScopeFolder.IsChecked == true ? SavedSearchScope.Folder
        : ScopePc.IsChecked == true ? SavedSearchScope.ThisPc
        : SavedSearchScope.CurrentFolder;

    private void Field_TextChanged(object sender, TextChangedEventArgs e) => Revalidate();

    private void Scope_Checked(object sender, RoutedEventArgs e) => Revalidate();

    private void Revalidate()
    {
        if (!_ready) return; // radios check before the fields exist

        var name = NameBox.Text.Trim();
        var scope = Scope;
        var scopePath = scope == SavedSearchScope.Folder ? _folder : null;

        // A new search may take an existing name (it replaces); an edit may not.
        var replacing = _editingName is null && name.Length > 0 && _nameTaken(name);
        var problem = SavedSearchRules.Validate(
            name, QueryBox.Text, scope, scopePath,
            _editingName is null ? _ => false : _nameTaken,
            File.Exists);

        ProblemText.Text = problem ?? "";
        ProblemText.Visibility = problem is null ? Visibility.Collapsed : Visibility.Visible;
        OkButton.Content = replacing ? "Replace" : "Save";
        OkButton.IsEnabled = problem is null;

        _result = problem is null ? new SavedSearch(name, QueryBox.Text.Trim(), scope, scopePath) : null;
    }

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        Revalidate();
        if (!OkButton.IsEnabled) return;
        DialogResult = true;
    }
}
