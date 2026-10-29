using System.Collections.ObjectModel;
using System.Globalization;
using System.IO;
using System.Windows.Media;
using BertBrowser.App.Interop;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Metadata;
using BertBrowser.Core.Services.Timestamps;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace BertBrowser.App.ViewModels;

/// <summary>Where one item of the selection stands.</summary>
public enum MetadataFileState
{
    Ready,
    /// <summary>Holds no metadata this app can edit, and says why. Its dates still can be.</summary>
    Skipped,
    Changed,
    Failed,
}

/// <summary>One selected item in the pane's list.</summary>
public sealed partial class MetadataFileRowViewModel(string path, bool isDirectory) : ObservableObject
{
    public string FullPath { get; } = path;

    public string Name { get; } = Path.GetFileName(path);

    public bool IsDirectory { get; } = isDirectory;

    public ImageSource? Icon => ShellIcons.GetIcon(FullPath, IsDirectory);

    [ObservableProperty]
    private MetadataFileState _state;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasDetail))]
    private string _detail = "";

    public bool HasDetail => Detail.Length > 0;

    /// <summary>What the file held when it was read, or null when it holds nothing editable.</summary>
    public MetadataDocument? Document { get; set; }

    /// <summary>The fields this file's format has room for.</summary>
    public IReadOnlySet<MetadataField> Fields { get; set; } = new HashSet<MetadataField>();

    /// <summary>Whether a cover picture can be embedded in it.</summary>
    public bool HoldsPicture { get; set; }

    /// <summary>No codec here writes it; Windows' own property handler will. The one kind of
    /// edit whose result is checked by its fields alone, and the row says so.</summary>
    public bool ViaWindows { get; set; }

    /// <summary>The item's own dates on disk, in canonical local form; empty when unreadable.</summary>
    public string Modified { get; set; } = "";

    /// <inheritdoc cref="Modified"/>
    public string Created { get; set; } = "";
}

/// <summary>
/// One field, across every selected item that has it.
/// </summary>
/// <remarks>
/// <b>A field is only written when its text was changed.</b> Where the items disagree the box
/// starts empty and says so, and leaving it alone leaves every item's own value alone — which is
/// what lets forty songs get one album without losing forty titles. Typing into it and then
/// emptying it again is a deliberate clear, and the row says that too, because the box looks the
/// same as it did before anything was typed.
/// </remarks>
public sealed partial class MetadataFieldRowViewModel : ObservableObject
{
    private readonly string _initial;
    private readonly Action _changed;
    private bool _touched;

    /// <param name="field">The metadata field, or null for one of the file's own dates.</param>
    public MetadataFieldRowViewModel(
        string id, string label, MetadataFieldKind kind, MetadataField? field,
        string initial, bool isMixed, Action changed)
    {
        Id = id;
        Label = label;
        Kind = kind;
        Field = field;
        _initial = initial;
        _changed = changed;
        IsMixed = isMixed;
        _text = initial;
    }

    public string Id { get; }

    public string Label { get; }

    public MetadataFieldKind Kind { get; }

    public MetadataField? Field { get; }

    /// <summary>The items hold different values for this field.</summary>
    public bool IsMixed { get; }

    /// <summary>Gets the calendar button beside its box.</summary>
    public bool IsDate => Kind == MetadataFieldKind.Date;

    /// <summary>How to type this kind of field, or null when there is nothing to say — an empty
    /// string would still pop up an empty tooltip.</summary>
    public string? Hint => Kind switch
    {
        MetadataFieldKind.Date => "A date and time, as 2024-03-14 09:26:53",
        MetadataFieldKind.List => "Separate each one with a semicolon",
        MetadataFieldKind.Rating => "Stars, 0 to 5",
        _ => null,
    };

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsDirty), nameof(IsInvalid), nameof(Note), nameof(ShowsNote),
        nameof(ShowsMixedPlaceholder), nameof(PickedDate))]
    private string _text;

    partial void OnTextChanged(string value)
    {
        _touched = true;
        _changed();
    }

    public bool ShowsMixedPlaceholder => IsMixed && !_touched && Text.Length == 0;

    private string? Normalized => MetadataFields.TryNormalize(Kind, Text, out var value) ? value : null;

    public bool IsDirty => IsMixed ? _touched : Normalized != _initial;

    /// <summary>Not a value of this kind — or an emptied date of the file's own, which can be
    /// changed but never cleared: there is no file without one.</summary>
    public bool IsInvalid => IsDirty && (Normalized is null || (Field is null && Normalized.Length == 0));

    /// <summary>The canonical value this row would write, when it is one.</summary>
    public string? Value => IsInvalid ? null : Normalized;

    public string Note
    {
        get
        {
            if (IsInvalid)
                return Kind switch
                {
                    MetadataFieldKind.Number => "Not a number",
                    MetadataFieldKind.Date => "Not a date",
                    MetadataFieldKind.Rating => "0 to 5",
                    _ => "Not allowed",
                };

            if (!IsDirty) return "";
            return Value is { Length: 0 } ? "Will be cleared" : "Changed";
        }
    }

    /// <summary>
    /// Whether the note earns a line of its own. Only where the box cannot say it: a value that
    /// will not be written, or a clear, which looks exactly like a box nobody touched. An ordinary
    /// change is already marked by the box and its revert button.
    /// </summary>
    public bool ShowsNote => IsInvalid || (IsDirty && Value is { Length: 0 });

    /// <summary>
    /// The day the calendar shows and sets. Picking one keeps the time already in the box — a
    /// calendar has no clock, and choosing a day must not move a photograph to midnight.
    /// </summary>
    public DateTime? PickedDate
    {
        get => MetadataFields.TryParseDate(Text.Trim(), out var date) ? date.Date : null;
        set
        {
            if (value is not { } day) return;

            var time = MetadataFields.TryParseDate(Text.Trim(), out var current) ? current.TimeOfDay
                : MetadataFields.TryParseDate(_initial, out var initial) ? initial.TimeOfDay
                : TimeSpan.Zero;

            Text = (day.Date + time).ToString(MetadataFields.DateFormat, CultureInfo.InvariantCulture);
        }
    }

    [RelayCommand]
    private void Revert()
    {
        Text = _initial;
        _touched = false;
        OnPropertyChanged(nameof(IsDirty));
        OnPropertyChanged(nameof(IsInvalid));
        OnPropertyChanged(nameof(Note));
        OnPropertyChanged(nameof(ShowsNote));
        OnPropertyChanged(nameof(ShowsMixedPlaceholder));
        _changed();
    }
}

/// <summary>
/// The metadata pane: one box per field over whatever is selected, and one undo step per apply.
/// </summary>
/// <remarks>
/// <para>
/// It reads through the same codecs that write, so what a box shows is exactly what would be
/// written back. A file no codec handles is read and written through Windows' own property
/// handler instead (<see cref="IPropertyFallback"/>) when that handler offers anything — still
/// one reader and writer per file — and its row says the result is not checked the way a
/// codec's is.
/// </para>
/// <para>
/// <b>It never writes.</b> Apply plans and executes through the shell, which is what puts an edit
/// behind the queue and into the undo history. The file's own modified and created dates are two
/// more boxes like any other; they go through the timestamp executor rather than a codec, after
/// the metadata is written, since writing it is itself what moves a modified date.
/// </para>
/// </remarks>
public sealed partial class MetadataViewModel : ObservableObject
{
    private const string ModifiedId = "modified";
    private const string CreatedId = "created";

    private readonly ShellViewModel _shell;
    private readonly IMetadataProbe _probe;
    private readonly IPropertyFallback _fallback;
    private int _generation;

    /// <summary>What the last apply did to each item, kept while the same items stay selected: the
    /// list reloads itself after every write, and a failure's reason must outlive that.</summary>
    private Dictionary<string, (MetadataFileState State, string Detail)> _results = new(StringComparer.OrdinalIgnoreCase);
    private string? _outcomeSummary;
    private IReadOnlyList<string> _paths = [];

    public MetadataViewModel(ShellViewModel shell, IMetadataProbe probe, IPropertyFallback fallback)
    {
        _shell = shell;
        _probe = probe;
        _fallback = fallback;
    }

    /// <summary>Raised after a write, with the paths that were selected for it, so the view can
    /// select them again in the reloaded list.</summary>
    public event Action<IReadOnlyList<string>>? Applied;

    public ObservableCollection<MetadataFileRowViewModel> Files { get; } = [];

    public ObservableCollection<MetadataFieldRowViewModel> Fields { get; } = [];

    [ObservableProperty]
    private string _title = "Metadata";

    [ObservableProperty]
    private string _summary = "";

    [ObservableProperty]
    private bool _isBusy;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsEmpty))]
    private bool _isLoading;

    /// <summary>Nothing is selected, so there is nothing to show but a line saying so.</summary>
    public bool IsEmpty => !IsLoading && Files.Count == 0;

    /// <summary>Completes when the selection has been read. For the harness, which has no other
    /// way to know the boxes are filled in.</summary>
    public Task Loaded { get; private set; } = Task.CompletedTask;

    /// <summary>Points the pane at a selection and reads it.</summary>
    public void Load(IReadOnlyList<string> paths)
    {
        if (!paths.SequenceEqual(_paths, StringComparer.OrdinalIgnoreCase))
        {
            _results = new(StringComparer.OrdinalIgnoreCase);
            _outcomeSummary = null;
        }

        _paths = [.. paths];
        Loaded = LoadAsync();
    }

    private async Task LoadAsync()
    {
        var generation = ++_generation;
        var paths = _paths;
        IsLoading = true;

        Title = paths.Count switch
        {
            0 => "Metadata",
            1 => Path.GetFileName(paths[0]) is { Length: > 0 } name ? name : paths[0],
            var n => $"{n} items",
        };

        var probe = _probe;
        var fallback = _fallback;
        var rows = await Task.Run(() => paths.Select(p => ReadOne(p, probe, fallback)).ToList());
        if (generation != _generation) return;

        foreach (var row in rows)
        {
            if (!_results.TryGetValue(row.FullPath, out var result)) continue;
            row.State = result.State;

            // A file Windows writes keeps saying so, whatever the last apply did to it.
            if (result.Detail.Length > 0 || !row.ViaWindows) row.Detail = result.Detail;
        }

        Files.Clear();
        foreach (var row in rows) Files.Add(row);

        // One file only: the shell describes a file, and there is nothing meaningful to merge.
        // And never a cloud placeholder or a link, which reading would download or follow.
        var details = rows is [{ IsDirectory: false } only] &&
                      probe.AttributesOf(only.FullPath) is { } attributes &&
                      Core.Services.Columns.MetadataReadRules.MayRead(attributes) &&
                      !MetadataEditPlanner.IsInsideArchive(only.FullPath)
            ? await Task.Run(() => Core.Services.Preview.PreviewMetadata.Details(
                [.. ShellProperties.Read(only.FullPath)
                    // What has a box below is not said twice: this list is for what nobody can type.
                    .Where(p => WindowsPropertyFields.FieldOf(p.Canonical) is not { } field || !only.Fields.Contains(field))
                    .Select(p => new Core.Services.Preview.ShellPropertyRow(p.Canonical, p.Name, p.Value))]))
            : [];
        if (generation != _generation) return;

        Details.Clear();
        foreach (var detail in details) Details.Add(detail);
        OnPropertyChanged(nameof(HasDetails));

        BuildFields();
        IsLoading = false;
        UpdateSummary();
    }

    /// <summary>Said under every file Windows writes rather than a codec.</summary>
    public const string ViaWindowsNote =
        "Written by Windows, not by this app: the fields are checked afterwards, the rest of the file is not.";

    private static MetadataFileRowViewModel ReadOne(string path, IMetadataProbe probe, IPropertyFallback fallback)
    {
        var attributes = probe.AttributesOf(path);
        var row = new MetadataFileRowViewModel(path, attributes?.HasFlag(FileAttributes.Directory) == true);

        try
        {
            FileSystemInfo info = row.IsDirectory ? new DirectoryInfo(path) : new FileInfo(path);
            if (info.Exists)
            {
                row.Modified = info.LastWriteTime.ToString(MetadataFields.DateFormat, CultureInfo.InvariantCulture);
                row.Created = info.CreationTime.ToString(MetadataFields.DateFormat, CultureInfo.InvariantCulture);
            }
        }
        catch (Exception ex) when (ReadOnlyFile.IsReadFailure(ex))
        {
        }

        if (row.IsDirectory)
        {
            row.State = MetadataFileState.Skipped;
            row.Detail = "A folder holds no metadata; only its dates can be changed.";
            return row;
        }

        if (MetadataEditPlanner.Refusal(path, attributes) is { } refusal)
        {
            // No codec is not the end of it: Windows may have a handler that writes this type.
            // Asked only once every other refusal has passed — asking opens the file.
            if (refusal.Reason == MetadataRejection.Unsupported &&
                MetadataEditPlanner.Refusal(path, attributes, viaWindows: true) is null &&
                fallback.Read(path) is { } offered)
            {
                row.Document = offered.Document;
                row.Fields = offered.Fields;
                row.ViaWindows = true;
                row.Detail = ViaWindowsNote;
                return row;
            }

            row.State = MetadataFileState.Skipped;
            row.Detail = refusal.Reason == MetadataRejection.Unsupported
                ? refusal.Message[..^1] + "; only its dates can be changed."
                : refusal.Message;
            return row;
        }

        try
        {
            using var stream = ReadOnlyFile.TryOpen(path);
            if (stream is null)
            {
                row.State = MetadataFileState.Skipped;
                row.Detail = $"{row.Name} could not be opened.";
                return row;
            }

            var codec = MetadataCodecs.For(path)!;
            row.Document = codec.Read(stream);
            row.Fields = codec.Fields;
            row.HoldsPicture = codec.HoldsPicture;
        }
        catch (MetadataFormatException ex)
        {
            row.State = MetadataFileState.Skipped;
            row.Detail = ex.Message;
        }
        catch (Exception ex) when (ReadOnlyFile.IsReadFailure(ex))
        {
            row.State = MetadataFileState.Skipped;
            row.Detail = $"{row.Name} could not be read: {ex.Message}";
        }

        return row;
    }

    /// <summary>The files a metadata edit would be offered: read, and not refused.</summary>
    private IEnumerable<MetadataFileRowViewModel> Editable => Files.Where(f => f.Document is not null);

    private void BuildFields()
    {
        Fields.Clear();
        var editable = Editable.ToList();

        foreach (var spec in MetadataFields.All)
        {
            var holders = editable.Where(f => f.Fields.Contains(spec.Field)).ToList();
            if (holders.Count == 0) continue;

            Add(spec.Id, spec.Label, spec.Kind, spec.Field, holders.Select(f => f.Document!.Get(spec.Field)));
        }

        // Every item has these, whatever it is — which is why they come last: on a text file or a
        // folder they are the whole pane.
        var dated = Files.Where(f => f.Modified.Length > 0).ToList();
        if (dated.Count > 0)
        {
            Add(ModifiedId, "Date modified", MetadataFieldKind.Date, null, dated.Select(f => f.Modified));
            Add(CreatedId, "Date created", MetadataFieldKind.Date, null, dated.Select(f => f.Created));
        }

        BuildPicture();
        OnPropertyChanged(nameof(IsEmpty));
        OnPropertyChanged(nameof(Located));
        OnPropertyChanged(nameof(LocationNote));
        OnPropertyChanged(nameof(CanRemove));
        NotifyCommands();
    }

    private void Add(string id, string label, MetadataFieldKind kind, MetadataField? field, IEnumerable<string> held)
    {
        var values = held.Distinct(StringComparer.Ordinal).ToList();
        var mixed = values.Count > 1;
        Fields.Add(new MetadataFieldRowViewModel(id, label, kind, field, mixed ? "" : values[0], mixed, OnFieldChanged));
    }

    private void NotifyCommands()
    {
        ApplyCommand.NotifyCanExecuteChanged();
        RemoveLocationCommand.NotifyCanExecuteChanged();
        RemoveAllCommand.NotifyCanExecuteChanged();
        RemovePictureCommand.NotifyCanExecuteChanged();
    }

    // --- the cover picture ---

    /// <summary>Whether anything selected can have a cover picture embedded in it.</summary>
    public bool CanHoldPicture => Editable.Any(f => f.HoldsPicture);

    /// <summary>The cover the selection shares, or null when there is none or they differ.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasPicture))]
    private ImageSource? _picture;

    public bool HasPicture => Picture is not null;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasPictureNote))]
    private string _pictureNote = "";

    public bool HasPictureNote => PictureNote.Length > 0;

    /// <summary>The picture's own bytes, for saving it out as the image file it is.</summary>
    public byte[]? PictureBytes { get; private set; }

    private void BuildPicture()
    {
        var holders = Editable.Where(f => f.HoldsPicture).ToList();
        var covers = holders.Select(f => f.Document!.Picture).ToList();
        var shared = covers.Count > 0 && covers[0] is { } first &&
                     covers.All(c => c is not null && c.AsSpan().SequenceEqual(first))
            ? first
            : null;

        PictureBytes = shared;
        Picture = shared is null ? null : Decode(shared);
        PictureNote = holders.Count == 0 ? ""
            : shared is not null && Picture is null ? "The cover picture is in a format that cannot be shown."
            : shared is not null ? ""
            : covers.All(c => c is null) ? "No cover picture."
            : "(multiple pictures)";

        OnPropertyChanged(nameof(CanHoldPicture));
    }

    private static ImageSource? Decode(byte[] bytes)
    {
        try
        {
            var image = new System.Windows.Media.Imaging.BitmapImage();
            image.BeginInit();
            image.CacheOption = System.Windows.Media.Imaging.BitmapCacheOption.OnLoad;
            image.DecodePixelWidth = 320;
            image.StreamSource = new MemoryStream(bytes);
            image.EndInit();
            image.Freeze();
            return image;
        }
        catch (Exception ex) when (ex is NotSupportedException or IOException or ArgumentException
                                       or InvalidOperationException or System.Runtime.InteropServices.COMException)
        {
            // Not a picture any installed decoder knows. Still embedded, still removable.
            return null;
        }
    }

    /// <summary>Embeds an image file as the cover of everything selected that can hold one.</summary>
    public Task SetPictureAsync(byte[] image) =>
        IsBusy ? Task.CompletedTask : RunAsync(MetadataEdit.None with { SetPicture = image }, null, null);

    private bool CanRemovePicture() => !IsBusy && Editable.Any(f => f.HoldsPicture && f.Document!.Picture is not null);

    [RelayCommand(CanExecute = nameof(CanRemovePicture))]
    private Task RemovePictureAsync() => RunAsync(MetadataEdit.None with { RemovePicture = true }, null, null);

    // --- read-only details ---

    /// <summary>
    /// What Windows can tell about the one selected file that nobody can type: its dimensions,
    /// its duration, its bit rate. Facts about the data rather than tags in it.
    /// </summary>
    public ObservableCollection<Core.Services.Preview.MetadataRow> Details { get; } = [];

    public bool HasDetails => Details.Count > 0;

    partial void OnIsBusyChanged(bool value) => NotifyCommands();

    private void OnFieldChanged()
    {
        ApplyCommand.NotifyCanExecuteChanged();
        _outcomeSummary = null;
        UpdateSummary();
    }

    private void UpdateSummary()
    {
        if (_outcomeSummary is { } outcome)
        {
            Summary = outcome;
            return;
        }

        if (Fields.Any(f => f.IsInvalid))
        {
            Summary = "Fix the highlighted field before applying.";
            return;
        }

        var dirty = Fields.Count(f => f.IsDirty);
        Summary = dirty switch
        {
            0 => "",
            1 => "1 field will be written.",
            _ => $"{dirty} fields will be written.",
        };
    }

    /// <summary>The metadata edit the boxes describe: every changed field, and nothing else.</summary>
    public MetadataEdit CurrentEdit() =>
        new(Fields.Where(f => f.Field is not null && f.IsDirty && f.Value is not null)
            .ToDictionary(f => f.Field!.Value, f => f.Value!));

    private bool CanApply() =>
        !IsBusy && Fields.Any(f => f.IsDirty) && !Fields.Any(f => f.IsInvalid);

    [RelayCommand(CanExecute = nameof(CanApply))]
    private Task ApplyAsync()
    {
        DateTime? Dirty(string id) =>
            Fields.FirstOrDefault(f => f.Id == id) is { IsDirty: true, Value: { Length: > 0 } value } &&
            MetadataFields.TryParseDate(value, out var date)
                ? date
                : null;

        return RunAsync(CurrentEdit(), Dirty(ModifiedId), Dirty(CreatedId));
    }

    // --- removing ---

    /// <summary>How many of the selected pictures say where they were taken.</summary>
    public int Located => Editable.Count(f => f.Document!.HasLocation);

    /// <summary>What a removal reaches: only what a codec writes. Taking something out is a
    /// promise about the whole file, and Windows' handlers are not read behind.</summary>
    private IEnumerable<MetadataFileRowViewModel> Removable => Editable.Where(f => !f.ViaWindows);

    public bool CanRemove => Removable.Any();

    public string LocationNote => Located switch
    {
        0 => "Nothing here says where it was made.",
        1 when Editable.Count() == 1 => "This picture says where it was taken.",
        1 => "1 of these says where it was taken.",
        var n => $"{n} of these say where they were taken.",
    };

    private bool CanRemoveLocation() => !IsBusy && Located > 0;

    private bool CanRemoveAll() => !IsBusy && Removable.Any();

    /// <summary>Takes the GPS position out of every picture that has one, and nothing else.</summary>
    [RelayCommand(CanExecute = nameof(CanRemoveLocation))]
    private Task RemoveLocationAsync() => RunAsync(MetadataEdit.None with { RemoveLocation = true }, null, null);

    /// <summary>Takes everything out. What the boxes hold is not written first: the two are
    /// separate things to ask for, and pressing this with a half-typed title means this.</summary>
    [RelayCommand(CanExecute = nameof(CanRemoveAll))]
    private Task RemoveAllAsync() => RunAsync(MetadataEdit.None with { RemoveAll = true }, null, null);

    private async Task RunAsync(MetadataEdit edit, DateTime? modified, DateTime? created)
    {
        if (edit.IsEmpty && modified is null && created is null) return;

        IsBusy = true;
        try
        {
            var results = new Dictionary<string, (MetadataFileState, string)>(StringComparer.OrdinalIgnoreCase);
            foreach (var file in Files.Where(f => f.State == MetadataFileState.Skipped))
                results[file.FullPath] = (MetadataFileState.Skipped, file.Detail);

            var failed = 0;
            var busy = false;

            if (!edit.IsEmpty)
            {
                var targets = (edit.Changes.Count > 0 ? Editable : Removable).ToList();
                var plan = _shell.PlanMetadataEdit(
                    [.. targets.Select(f => f.FullPath)],
                    edit,
                    targets.Where(f => f.ViaWindows)
                        .ToDictionary(f => f.FullPath, f => f.Fields, StringComparer.OrdinalIgnoreCase));
                foreach (var rejected in plan.Rejected) results[rejected.Path] = (MetadataFileState.Ready, rejected.Message);

                if (plan.HasWork)
                {
                    if (await _shell.EditMetadataAsync(plan) is { } outcome)
                    {
                        foreach (var edited in outcome.Edited) results[edited.Path] = (MetadataFileState.Changed, "");
                        foreach (var failure in outcome.Failed) results[failure.Path] = (MetadataFileState.Failed, failure.Message);
                        failed += outcome.Failed.Count;
                    }
                    else
                    {
                        busy = true;
                    }
                }
            }

            // After the metadata, never before: rewriting a file is what moves its modified date,
            // so a date set first would be overwritten by the very edit it was applied with.
            foreach (var (value, isModified) in new[] { (modified, true), (created, false) })
            {
                if (value is not { } date || busy) continue;

                var plan = _shell.PlanTimestamps(
                    [.. Files.Select(f => f.FullPath)],
                    new TimestampChange(isModified, !isModified, TimestampSource.Value, Value: date));
                if (!plan.HasWork) continue;

                // The queue lowers its flag a moment after the edit's own task completes.
                for (var i = 0; i < 40 && _shell.IsTransferring; i++) await Task.Delay(50);

                if (await _shell.SetTimestampsAsync(plan) is { } outcome)
                {
                    foreach (var stamped in outcome.Stamped)
                    {
                        if (!results.TryGetValue(stamped.Path, out var had) || had.Item1 != MetadataFileState.Failed)
                            results[stamped.Path] = (MetadataFileState.Changed, "");
                    }
                    foreach (var failure in outcome.Failed) results[failure.Path] = (MetadataFileState.Failed, failure.Message);
                    failed += outcome.Failed.Count;
                }
                else
                {
                    busy = true;
                }
            }

            _results = results;
            var touched = results.Count(r => r.Value.Item1 == MetadataFileState.Changed);
            _outcomeSummary = busy
                ? "The app is busy with another operation. Try again when it finishes."
                : touched == 0 && failed == 0 ? "Nothing needed changing."
                : failed > 0 ? $"{Count(touched)} changed; {failed} could not be. Undo takes it back."
                : $"{Count(touched)} changed. Undo takes it back.";

            // The shell reloaded the list behind this, and its rows are new objects with nothing
            // selected: put the selection back, or the pane would answer its own edit by going blank.
            Applied?.Invoke(_paths);

            // Read again rather than patched in place: the boxes then show what the items hold now,
            // which for a failed one is what it held before.
            await LoadAsync();
        }
        finally
        {
            IsBusy = false;
        }
    }

    private static string Count(int items) => items == 1 ? "1 item" : $"{items} items";
}
