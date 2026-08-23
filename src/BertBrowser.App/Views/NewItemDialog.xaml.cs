using System.IO;
using System.Windows;
using System.Windows.Controls;
using BertBrowser.Core.Services.NewItem;

namespace BertBrowser.App.Views;

/// <summary>
/// Asks what to call a new folder or file.
/// </summary>
/// <remarks>
/// It re-plans on every keystroke rather than validating the text itself, so a name that is illegal
/// or already taken is refused here — while it can still be changed — by exactly the rule the
/// create will obey. Nothing is written until Create is pressed, so cancelling leaves no litter.
/// </remarks>
public partial class NewItemDialog : ThemedWindow
{
    private readonly string _directory;
    private readonly NewItemKind _kind;
    private readonly NewFileTemplate? _template;
    private readonly Func<string, string, NewItemKind, string?, NewItemPlan> _planner;

    private NewItemPlan _plan = NewItemPlan.Empty;

    private NewItemDialog(
        string directory,
        NewItemKind kind,
        NewFileTemplate? template,
        string initialName,
        Func<string, string, NewItemKind, string?, NewItemPlan> planner)
    {
        InitializeComponent();
        _directory = directory;
        _kind = kind;
        _template = template;
        _planner = planner;

        var what = kind == NewItemKind.Folder ? "folder" : template?.Label ?? "file";
        Title = $"New {what}";
        PromptText.Text = $"Name for the new {what}:";
        LocationText.Text = $"In: {directory}";

        NameBox.Text = initialName;
        Replan();
        Loaded += (_, _) =>
        {
            NameBox.Focus();
            // Select the part that gets typed over, leaving a file's extension alone.
            NameBox.Select(0, NewItemPattern.StemLength(NameBox.Text, kind));
        };
    }

    /// <summary>The dialog built but not shown, for the UI harness to park offscreen and
    /// photograph. Nothing in the app uses it: <see cref="Show"/> is the only way in from the
    /// interface, and it goes through this same constructor.</summary>
    internal static NewItemDialog Create(
        string directory,
        NewItemKind kind,
        NewFileTemplate? template,
        string initialName,
        Func<string, string, NewItemKind, string?, NewItemPlan> planner) =>
        new(directory, kind, template, initialName, planner);

    /// <summary>Shows the dialog and returns the plan to carry out, or null if it was cancelled or
    /// there was nothing to do.</summary>
    public static NewItemPlan? Show(
        Window? owner,
        string directory,
        NewItemKind kind,
        NewFileTemplate? template,
        string initialName,
        Func<string, string, NewItemKind, string?, NewItemPlan> planner)
    {
        if (directory.Length == 0) return null;

        var dialog = new NewItemDialog(directory, kind, template, initialName, planner);
        if (owner is not null && !ReferenceEquals(owner, dialog)) dialog.Owner = owner;

        return dialog.ShowDialog() == true ? dialog._plan : null;
    }

    private void NameBox_TextChanged(object sender, TextChangedEventArgs e) => Replan();

    private void Replan()
    {
        _plan = _planner(_directory, TypedName(), _kind, _template?.TemplatePath);

        var problem = _plan.Rejected;
        ProblemText.Text = problem?.Message ?? "";
        ProblemText.Visibility = problem is null ? Visibility.Collapsed : Visibility.Visible;

        OkButton.IsEnabled = _plan.HasWork;
    }

    /// <summary>What the user typed, with the type's extension put back if they removed it. Typing
    /// over the pre-selected stem leaves the extension in the box, so this only does anything when
    /// they deliberately cleared it — and a "New JSON File" that produced no ".json" would be a
    /// surprise, while one that refuses to let them type ".txt" instead would be worse.</summary>
    private string TypedName()
    {
        var typed = NameBox.Text;
        if (_kind == NewItemKind.Folder) return typed;
        if (_template is not { Extension.Length: > 0 } template) return typed;

        return Path.GetExtension(typed).Length > 0 ? typed : typed + template.Extension;
    }

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        Replan(); // disk may have changed while the dialog sat open
        if (!OkButton.IsEnabled) return;
        DialogResult = true;
    }
}
