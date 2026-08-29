using System.Globalization;
using System.Text;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Threading;
using BertBrowser.App.Services;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Rename;
using Microsoft.Extensions.DependencyInjection;

namespace BertBrowser.App.Views;

/// <summary>
/// Asks for the new name. One item is renamed to exactly what is typed; several are numbered from
/// it — "Holiday" over three photos gives "Holiday 1", "Holiday 2", "Holiday 3", each keeping its
/// own extension. "More options" opens a panel adding find/replace, a case transform, a counter
/// and a date, placed by tokens in the same box.
/// </summary>
/// <remarks>
/// The dialog re-plans on every keystroke rather than validating the text on its own, so what it
/// previews is what the rename will do, and a name that is already taken is refused here — before
/// anything is written — instead of failing halfway through a batch.
///
/// <para>The two states are deliberately different rules, not one rule with more knobs. Collapsed,
/// the box is <see cref="RenameRule.Simple"/>: taken literally, braces and all, exactly as it has
/// always been, because "{6B99A0C1}.tmp" is an ordinary name to give a file. Tokens and the
/// persisted options apply only once the panel is open and the list of them is on screen.</para>
/// </remarks>
public partial class RenameDialog : ThemedWindow
{
    /// <summary>How many "old → new" lines the collapsed preview lists before summarising.</summary>
    private const int PreviewLines = 6;

    private const double CollapsedWidth = 460;
    private const double ExpandedWidth = 760;

    /// <summary>The guard the search box already uses. A regular expression gets 250 ms per name,
    /// so re-planning a large selection on every keystroke is otherwise a frozen window.</summary>
    private static readonly TimeSpan ReplanDelay = TimeSpan.FromMilliseconds(200);

    private readonly IReadOnlyList<RenameSource> _sources;
    private readonly Func<IReadOnlyList<RenameSource>, RenameRule, RenamePlan> _planner;
    private readonly AppSettings? _settings;
    private readonly DispatcherTimer _replan;

    /// <summary>What the box held before anything was typed, so an untouched suggestion can be
    /// recognised and re-seeded — and put back if the panel is closed again.</summary>
    private readonly string _suggestion;

    private RenamePlan _plan = RenamePlan.Empty;
    private bool _expanded;
    private string _seeded = "";
    private bool _loading;

    private RenameDialog(
        IReadOnlyList<RenameSource> sources,
        Func<IReadOnlyList<RenameSource>, RenameRule, RenamePlan> planner,
        bool expanded)
    {
        InitializeComponent();
        _sources = sources;
        _planner = planner;
        _settings = App.Services?.GetService<AppSettings>();

        _replan = new DispatcherTimer { Interval = ReplanDelay };
        _replan.Tick += (_, _) => { _replan.Stop(); Replan(); };

        _suggestion = RenamePattern.SuggestFor(sources);
        NameBox.Text = _suggestion;

        FillChoices();
        LoadOptions();

        if (expanded || _settings?.AdvancedRenameExpanded == true) Expand();
        else Prompt();

        Replan(); // the suggestion itself may be a no-op, which must not leave Rename enabled
        Loaded += (_, _) => PlaceCaret();
    }

    /// <summary>The dialog built but not shown, for the UI harness to park offscreen and
    /// photograph. Nothing in the app uses it: <see cref="Show"/> is the only way in from the
    /// interface, and both go through the same constructor so a capture cannot drift from what
    /// the app puts on screen.</summary>
    internal static RenameDialog Create(
        IReadOnlyList<RenameSource> sources,
        Func<IReadOnlyList<RenameSource>, RenameRule, RenamePlan> planner,
        bool expanded = false) => new(sources, planner, expanded);

    /// <summary>Shows the dialog and returns the plan to carry out, or null if it was cancelled or
    /// there was nothing to do.</summary>
    public static RenamePlan? Show(
        Window? owner,
        IReadOnlyList<RenameSource> sources,
        Func<IReadOnlyList<RenameSource>, RenameRule, RenamePlan> planner)
    {
        if (sources.Count == 0) return null;

        var dialog = new RenameDialog(sources, planner, expanded: false);
        if (owner is not null && !ReferenceEquals(owner, dialog)) dialog.Owner = owner;

        return dialog.ShowDialog() == true ? dialog._plan : null;
    }

    // --- the options panel ---

    private void Options_Click(object sender, RoutedEventArgs e)
    {
        if (_expanded) Collapse(); else Expand();

        // CenterOwner fired at the old size, so growing would otherwise push the dialog down and
        // right of where it was — off the bottom of a small screen, in the worst case.
        Dispatcher.BeginInvoke(DispatcherPriority.Loaded, Recentre);

        Replan();
        PlaceCaret();
    }

    private void Expand()
    {
        _expanded = true;
        OptionsPanel.Visibility = Visibility.Visible;
        PreviewBox.Visibility = Visibility.Visible;
        PreviewText.Visibility = Visibility.Collapsed;
        OptionsToggle.Content = "Fewer options";
        Width = ExpandedWidth;
        Prompt();

        // Seeding matters: a literal template is a find/replace with nothing to act on, and the
        // box is holding a literal — the suggestion — every time the panel is first opened.
        // Anything the user has already typed is theirs and is left alone.
        if (!string.Equals(NameBox.Text, _suggestion, StringComparison.Ordinal)) return;

        // The seed reproduces what the collapsed box was about to do, so opening the panel never
        // changes the pending result or greys out Rename.
        _seeded = _sources.Count > 1 ? Escape(_suggestion) + " {n}{ext}" : "{name}";
        NameBox.Text = _seeded;
    }

    private void Collapse()
    {
        _expanded = false;
        OptionsPanel.Visibility = Visibility.Collapsed;
        PreviewBox.Visibility = Visibility.Collapsed;
        OptionsToggle.Content = "More options";
        Width = CollapsedWidth;
        Prompt();

        // Only what this dialog put there is taken back; anything typed since is the user's.
        if (_seeded.Length > 0 && string.Equals(NameBox.Text, _seeded, StringComparison.Ordinal))
            NameBox.Text = _suggestion;
        _seeded = "";
    }

    /// <summary>What the box is asking for, which is not the same question in the two states:
    /// collapsed it takes a name and numbers a batch, expanded it takes a template that numbers
    /// only where a counter was placed. Promising numbering either way would be a lie in one of
    /// them.</summary>
    private void Prompt() => PromptText.Text = (_expanded, _sources.Count) switch
    {
        (true, 1) => "Name template:",
        (true, var n) => $"Name template for {n:N0} items:",
        (false, 1) => "New name:",
        (false, var n) => $"New name for {n:N0} items — they will be numbered:",
    };

    /// <summary>Puts the caret where the next keystroke should land: over the part of a single
    /// name a rename usually replaces, or over the whole template.</summary>
    /// <remarks>Re-placed after the panel opens, or the selection left by the initial pass sits
    /// over the seeded template and the next keystroke wipes it.</remarks>
    private void PlaceCaret()
    {
        NameBox.Focus();
        if (_expanded || _sources.Count != 1) NameBox.SelectAll();
        else NameBox.Select(0, RenamePattern.BaseNameLength(_sources[0]));
    }

    private void Recentre()
    {
        if (Owner is not { } owner || !IsLoaded) return;
        UpdateLayout();
        // Deliberately not clamped to the work area: the harness parks both windows far offscreen
        // and a clamp would drag the dialog onto the user's actual desktop.
        Left = owner.Left + ((owner.ActualWidth - ActualWidth) / 2);
        Top = owner.Top + ((owner.ActualHeight - ActualHeight) / 2);
    }

    private void FillChoices()
    {
        _loading = true;
        ScopeBox.ItemsSource = new[]
        {
            new Choice("Name, without extension", (int)RenameScope.Stem),
            new Choice("Extension only", (int)RenameScope.Extension),
            new Choice("Name and extension", (int)RenameScope.WholeName),
        };
        CaseBox.ItemsSource = new[]
        {
            new Choice("As typed", (int)RenameCase.AsIs),
            new Choice("lower case", (int)RenameCase.Lower),
            new Choice("UPPER CASE", (int)RenameCase.Upper),
            new Choice("Title Case", (int)RenameCase.Title),
            new Choice("Sentence case", (int)RenameCase.Sentence),
        };
        ScopeBox.SelectedIndex = 0;
        CaseBox.SelectedIndex = 0;
        StartBox.Text = "1";
        StepBox.Text = "1";
        _loading = false;
    }

    private void LoadOptions()
    {
        if (_settings?.AdvancedRename is not { } saved) return;

        _loading = true;
        RegexCheck.IsChecked = saved.UseRegex;
        MatchCaseCheck.IsChecked = saved.MatchCase;
        ScopeBox.SelectedIndex = (int)saved.Scope;
        CaseBox.SelectedIndex = (int)saved.Case;
        StartBox.Text = saved.CounterStart.ToString(CultureInfo.InvariantCulture);
        StepBox.Text = saved.CounterStep.ToString(CultureInfo.InvariantCulture);
        _loading = false;
    }

    /// <summary>Keeps the knobs and drops the text: a regular expression from last week waiting
    /// behind F2 is a trap, but having to set the counter width again every time is a nuisance.</summary>
    private void SaveOptions()
    {
        if (_settings is null) return;

        _settings.AdvancedRenameExpanded = _expanded;
        if (_expanded)
            _settings.AdvancedRename = CurrentRule() with { Template = "", Find = "", Replace = "" };
        _settings.Save();
    }

    // --- planning ---

    private RenameRule CurrentRule()
    {
        if (!_expanded) return RenameRule.Simple(NameBox.Text);

        return new RenameRule(
            NameBox.Text,
            Find: FindBox.Text,
            Replace: ReplaceBox.Text,
            UseRegex: RegexCheck.IsChecked == true,
            MatchCase: MatchCaseCheck.IsChecked == true,
            Scope: (RenameScope)Chosen(ScopeBox),
            Case: (RenameCase)Chosen(CaseBox),
            CounterStart: Number(StartBox.Text, 1),
            CounterStep: Number(StepBox.Text, 1));
    }

    private static int Chosen(ComboBox box) => (box.SelectedItem as Choice)?.Value ?? 0;

    private void NameBox_TextChanged(object sender, TextChangedEventArgs e) => ReplanSoon();

    private void Option_Changed(object sender, TextChangedEventArgs e) => ReplanSoon();

    private void Option_Toggled(object sender, RoutedEventArgs e) => ReplanSoon();

    private void Option_Selected(object sender, SelectionChangedEventArgs e) => ReplanSoon();

    /// <summary>Re-plans, after a pause once the panel is open. Collapsed there is nothing to wait
    /// for — the plain rule is a string concatenation — so that path stays as immediate as it was.</summary>
    private void ReplanSoon()
    {
        if (_loading) return;
        if (!_expanded) { Replan(); return; }
        _replan.Stop();
        _replan.Start();
    }

    private void Replan()
    {
        _replan.Stop();

        var numbersOk = Numeric(StartBox.Text) && Numeric(StepBox.Text);
        _plan = _planner(_sources, CurrentRule());

        // Any refusal blocks the whole rename: a batch that silently skips some of its items is
        // worse than one that says what is wrong while the name can still be changed.
        var problem = _plan.Rejected.FirstOrDefault()?.Message;
        if (_expanded && !numbersOk) problem = "Counter start and step have to be whole numbers.";

        ProblemText.Text = problem ?? "";
        ProblemText.Visibility = problem is null ? Visibility.Collapsed : Visibility.Visible;

        var hint = _expanded ? Hint() : null;
        HintText.Text = hint ?? "";
        HintText.Visibility = hint is null ? Visibility.Collapsed : Visibility.Visible;

        if (_expanded) PreviewList.ItemsSource = Rows();
        else
        {
            PreviewText.Text = Preview();
            PreviewText.Visibility = PreviewText.Text.Length == 0
                ? Visibility.Collapsed
                : Visibility.Visible;
        }

        OkButton.IsEnabled = problem is null && _plan.HasWork;
    }

    /// <summary>One row per selected item, whatever the plan made of it.</summary>
    /// <remarks>
    /// Built from the selection rather than from the plan, because a plan is not always one entry
    /// per item: a rule that cannot be used at all comes back as a single refusal with no source
    /// path, and merging the plan's own two lists would then show one row where there are three
    /// hundred items. That refusal belongs in the banner and matches nothing here.
    /// </remarks>
    private List<PreviewRow> Rows()
    {
        var planned = new Dictionary<string, PlannedRename>(StringComparer.Ordinal);
        foreach (var rename in _plan.Renames)
            planned[Key(rename.SourcePath)] = rename;

        var refused = new Dictionary<string, RejectedRename>(StringComparer.Ordinal);
        foreach (var rejection in _plan.Rejected)
        {
            if (rejection.SourcePath.Length == 0) continue;
            refused.TryAdd(Key(rejection.SourcePath), rejection);
        }

        var rows = new List<PreviewRow>(_sources.Count);
        foreach (var source in _sources)
        {
            var key = Key(source.Path);
            if (planned.TryGetValue(key, out var rename))
                rows.Add(new PreviewRow(source.Name, rename.TargetName, null));
            else if (refused.TryGetValue(key, out var rejection))
                rows.Add(new PreviewRow(source.Name, "", rejection.Message));
            else
                rows.Add(new PreviewRow(source.Name, source.Name, null));
        }
        return rows;
    }

    /// <summary>Something the rule will do that was probably not meant. Not a refusal — the names
    /// are legal and the preview shows exactly what they will be.</summary>
    private string? Hint()
    {
        var segments = RenameTemplate.Parse(NameBox.Text.Trim(), out _);
        if (segments is null) return null;

        var names = RenameTemplate.Uses(segments, RenamePart.Name, RenamePart.Base, RenamePart.Extension);

        if (FindBox.Text.Length > 0 && !names)
            return "Find and replace has nothing to act on — the name has no {name}, {base} or {ext} in it.";

        // Numbering by hand loses what the plain box added for free. "Holiday {n}" over a folder
        // of photos silently drops every extension.
        var keepsExtension = RenameTemplate.Uses(segments, RenamePart.Name, RenamePart.Extension);
        if (RenameTemplate.Uses(segments, RenamePart.Counter) && !keepsExtension &&
            _sources.Any(s => RenamePattern.Split(s).Extension.Length > 0))
            return "These names will lose their extensions — add {ext} to keep them.";

        return null;
    }

    /// <summary>"old → new" for the first few items, then a count for the rest. A single item shows
    /// nothing: the text box already says what it will be called.</summary>
    private string Preview()
    {
        if (_sources.Count < 2) return "";

        var work = _plan.Renames;
        if (work.Count == 0) return "";

        var text = new StringBuilder();
        foreach (var rename in work.Take(PreviewLines))
        {
            if (text.Length > 0) text.Append('\n');
            text.Append(rename.SourceName).Append("  →  ").Append(rename.TargetName);
        }
        if (work.Count > PreviewLines)
            text.Append($"\n…and {work.Count - PreviewLines:N0} more");
        return text.ToString();
    }

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        Replan(); // disk may have changed while the dialog sat open
        if (!OkButton.IsEnabled) return;
        SaveOptions();
        DialogResult = true;
    }

    private static string Key(string path)
    {
        try
        {
            return PathKey.Canonicalize(path);
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
        {
            return path;
        }
    }

    /// <summary>Doubles any brace in text that is going into a template, so a file actually called
    /// "{draft}.txt" seeds a template that means itself.</summary>
    private static string Escape(string text) => text.Replace("{", "{{").Replace("}", "}}");

    private static bool Numeric(string text) =>
        int.TryParse(text, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out _);

    private static int Number(string text, int fallback) =>
        int.TryParse(text, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out var value)
            ? value
            : fallback;

    /// <param name="Label">What the list shows.</param>
    /// <param name="Value">The enum member it stands for.</param>
    private sealed record Choice(string Label, int Value);

    /// <param name="Name">The item's name now.</param>
    /// <param name="NewName">What it would be called, or empty when it was refused.</param>
    /// <param name="Problem">Why it was refused, or null.</param>
    private sealed record PreviewRow(string Name, string NewName, string? Problem)
    {
        public string Display => Problem ?? NewName;

        public bool IsProblem => Problem is not null;

        public bool IsUnchanged =>
            Problem is null && string.Equals(Name, NewName, StringComparison.Ordinal);
    }
}
