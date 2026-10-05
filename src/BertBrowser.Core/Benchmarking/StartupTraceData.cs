using System.Text.Json;
using System.Text.Json.Serialization;

namespace BertBrowser.Core.Benchmarking;

/// <summary>The marks the app writes on its way up, as milliseconds since the process started.</summary>
public static class StartupMarks
{
    public const string Main = "main";
    public const string VelopackDone = "velopackDone";
    public const string InstanceClaimed = "instanceClaimed";

    /// <summary>App.xaml and the dictionaries it merges loaded, on the UI thread.</summary>
    public const string AppXamlLoaded = "appXamlLoaded";

    /// <summary>settings.json parsed — the composition root waits on it before anything else.</summary>
    public const string SettingsLoaded = "settingsLoaded";
    public const string ServicesBuilt = "servicesBuilt";
    public const string Migrated = "migrated";
    public const string ThemeReady = "themeReady";

    /// <summary>The shell view model and the graph behind it resolved.</summary>
    public const string ShellBuilt = "shellBuilt";

    /// <summary>The main window's own XAML loaded, before any pane exists.</summary>
    public const string WindowXamlLoaded = "windowXamlLoaded";

    /// <summary>The first pane and its tab view built.</summary>
    public const string PanesBuilt = "panesBuilt";
    public const string CommandsBuilt = "commandsBuilt";

    /// <summary>The window constructed; what is left before <see cref="WindowShown"/> is Show().</summary>
    public const string WindowBuilt = "windowBuilt";
    public const string WindowShown = "windowShown";
    public const string ContentRendered = "contentRendered";
    public const string FirstListing = "firstListing";
}

/// <summary>
/// One launch of the real executable, as the app itself timed it.
/// </summary>
/// <param name="MarksMs">Mark name to milliseconds since process start. <see cref="StartupMarks"/>.</param>
/// <param name="Instance"><c>first</c> or <c>second</c>: whether the single-instance claim won, which
/// says whether another copy was running during the measurement.</param>
/// <param name="Indexer">What the app did about the index helper — <c>skipped</c> under a trace.</param>
/// <param name="SideEffectsSkipped">True when the post-window work (helper attach, change-log policy,
/// update check, staging sweep) was left out, so the numbers describe "to first listing".</param>
/// <param name="Error">Non-null when a mark never came and the fallback flushed what there was.</param>
/// <param name="Memory">The process's memory once the first listing was up, by <see cref="StartupMemoryPoints"/>
/// name. Null in a trace written before this was recorded.</param>
public sealed record StartupTraceData(
    int Pid,
    DateTime ProcessStartUtc,
    IReadOnlyDictionary<string, double> MarksMs,
    string Instance,
    string Indexer,
    bool SideEffectsSkipped,
    string? Error,
    IReadOnlyDictionary<string, StartupMemory>? Memory = null);

/// <summary>The two moments a traced launch reads its own memory.</summary>
public static class StartupMemoryPoints
{
    /// <summary>As it stood when the first listing was shown — what Task Manager would say.</summary>
    public const string FirstListing = "firstListing";

    /// <summary>The same moment after a full collection — what the app is actually holding, and the
    /// steadier of the two.</summary>
    public const string Collected = "collected";
}

/// <summary>One reading of the real executable's memory, taken by the app itself.</summary>
public sealed record StartupMemory(
    long WorkingSetBytes,
    long PrivateBytes,
    long ManagedHeapBytes,
    long PeakWorkingSetBytes);

/// <summary>Reads and writes a trace file. The app writes; the benchmark tool reads.</summary>
public static class StartupTraceJson
{
    private static readonly JsonSerializerOptions Options = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        PropertyNameCaseInsensitive = true,
        WriteIndented = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };

    public static string Serialize(StartupTraceData trace) => JsonSerializer.Serialize(trace, Options);

    public static StartupTraceData Deserialize(string json) =>
        JsonSerializer.Deserialize<StartupTraceData>(json, Options)
        ?? throw new InvalidDataException("Not a startup trace: empty document.");

    public static void Write(StartupTraceData trace, string path)
    {
        if (Path.GetDirectoryName(Path.GetFullPath(path)) is { Length: > 0 } dir) Directory.CreateDirectory(dir);
        File.WriteAllText(path, Serialize(trace));
    }

    public static StartupTraceData Read(string path) => Deserialize(File.ReadAllText(path));
}
