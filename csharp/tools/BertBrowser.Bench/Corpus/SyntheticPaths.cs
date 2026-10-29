using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;

namespace BertBrowser.Bench.Corpus;

/// <summary>
/// One seeded, invented disk: the directory tree, every file's name, size, dates and attributes.
/// </summary>
/// <remarks>
/// <para>
/// The index corpus, the directory-size rows and the synthetic MFT are all cut from this one model,
/// so a search benchmark and an MFT benchmark are describing the same world. It lives on drive
/// <c>Q:</c>, which does not exist — <see cref="PathKey.Canonicalize"/> is <c>GetFullPath</c> plus
/// uppercasing and is happy with a rooted path to nowhere — so nothing here can ever be confused
/// with a real folder.
/// </para>
/// <para>
/// The shape approximates a developer's Windows machine: a few hundred thousand files, most of them
/// small, under <c>Users</c>, <c>Source</c>, <c>Windows</c> and <c>Program Files</c>. The documented
/// search queries all find something — <c>report</c>, <c>IMG_</c> names, files over 100 MB, files
/// modified "today" (a fixed date, so the corpus never ages) — and nothing is over 100 GB, so the
/// worst-case query still matches nothing. Same seed, same scale, same disk, every time.
/// </para>
/// </remarks>
internal sealed class SyntheticPaths
{
    public const int GeneratorVersion = 1;
    public const string Drive = @"Q:\";
    public const string SearchRoot = @"Q:\Source";

    /// <summary>The corpus's "today". The benchmark query is <c>dm:2026-09-15</c>, never <c>dm:today</c>.</summary>
    public static readonly DateTime Today = new(2026, 9, 15, 0, 0, 0, DateTimeKind.Utc);
    public const string TodayQuery = "dm:2026-09-15";

    private static readonly DateTime DateFloor = new(2021, 10, 1, 0, 0, 0, DateTimeKind.Utc);
    private static readonly DateTime DateCeiling = new(2026, 9, 30, 0, 0, 0, DateTimeKind.Utc);

    public int Scale { get; }

    /// <summary>Every row, directories first, each parent before its children.</summary>
    public IReadOnlyList<FsEntryRow> Rows { get; }
    public IReadOnlyList<FsEntryRow> Directories { get; }
    public IReadOnlyList<FsEntryRow> Files { get; }

    /// <summary>A <c>dir_size_cache</c> row for every directory, drive root included.</summary>
    public IReadOnlyList<DirSizeResult> DirectorySizes { get; }

    private SyntheticPaths(int scale, List<FsEntryRow> dirs, List<FsEntryRow> files, List<DirSizeResult> sizes)
    {
        Scale = scale;
        Directories = dirs;
        Files = files;
        Rows = [.. dirs, .. files];
        DirectorySizes = sizes;
    }

    public static SyntheticPaths Generate(int scale)
    {
        if (scale < 1) throw new ArgumentOutOfRangeException(nameof(scale));

        var rng = new Random(12345 + scale);
        var target = scale * 100_000;
        var dirTarget = Math.Max(target / 12, Skeleton.Length + 16);
        var fileTarget = target - dirTarget;

        var nodes = new List<DirNode>(dirTarget + 1) { new(Drive, 0, false, -1) };
        var byPath = new Dictionary<string, int>(StringComparer.OrdinalIgnoreCase) { [Drive] = 0 };

        foreach (var (relative, hidden) in Skeleton)
        {
            var path = Drive + relative;
            var parentPath = Path.GetDirectoryName(path)!;
            var parent = byPath[parentPath];
            var node = new DirNode(path, nodes[parent].Depth + 1, hidden || nodes[parent].Hidden, parent);
            byPath[path] = nodes.Count;
            nodes.Add(node);
        }

        while (nodes.Count <= dirTarget)
        {
            var parent = rng.Next(1, nodes.Count);
            if (nodes[parent].Depth >= 14) continue;

            var dotted = rng.NextDouble() < 0.015;
            var name = (dotted ? "." : "") + DirName(rng);
            var path = Path.Combine(nodes[parent].Path, name);
            if (!byPath.TryAdd(path, nodes.Count)) continue;

            nodes.Add(new DirNode(path, nodes[parent].Depth + 1, dotted || nodes[parent].Hidden, parent));
        }

        var files = new List<FsEntryRow>(fileTarget);
        var usedNames = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        for (var i = 0; i < fileTarget; i++)
        {
            var parentIndex = rng.Next(1, nodes.Count);
            var parent = nodes[parentIndex];
            var name = FileName(rng);
            var path = Path.Combine(parent.Path, name);
            if (!usedNames.Add(path))
            {
                i--;
                continue;
            }

            var size = FileSize(rng);
            var modified = Modified(rng);
            var created = rng.NextDouble() < 0.05 ? modified.AddDays(rng.Next(1, 400)) : modified;
            var hidden = parent.Hidden || rng.NextDouble() < 0.03;
            var attributes = FileAttributes.Archive;
            if (hidden) attributes |= FileAttributes.Hidden;
            if (rng.NextDouble() < 0.02) attributes |= FileAttributes.ReadOnly;
            if (rng.NextDouble() < 0.005) attributes |= FileAttributes.System;

            files.Add(new FsEntryRow(PathKey.Canonicalize(path), name, false, size, modified, hidden, attributes, created));

            parent.Bytes += size;
            parent.FileCount++;
        }

        // Roll each directory's totals into its parent. Children always follow their parents in the
        // list, so one reverse pass is a complete post-order fold.
        for (var i = nodes.Count - 1; i > 0; i--)
        {
            var node = nodes[i];
            var parent = nodes[node.Parent];
            parent.Bytes += node.Bytes;
            parent.FileCount += node.FileCount;
            parent.DirCount += node.DirCount + 1;
        }

        var dirs = new List<FsEntryRow>(nodes.Count - 1);
        var sizes = new List<DirSizeResult>(nodes.Count);
        var computed = Today.AddHours(1);
        foreach (var node in nodes)
        {
            sizes.Add(new DirSizeResult(PathKey.Canonicalize(node.Path), node.Bytes, node.FileCount, node.DirCount, false, computed));
            if (node.Depth == 0) continue;

            var modified = Modified(rng);
            var attributes = FileAttributes.Directory | (node.Hidden ? FileAttributes.Hidden : 0);
            dirs.Add(new FsEntryRow(PathKey.Canonicalize(node.Path), Path.GetFileName(node.Path), true, 0, modified, node.Hidden, attributes, modified));
        }

        return new SyntheticPaths(scale, dirs, files, sizes);
    }

    private sealed class DirNode(string path, int depth, bool hidden, int parent)
    {
        public string Path { get; } = path;
        public int Depth { get; } = depth;
        public bool Hidden { get; } = hidden;
        public int Parent { get; } = parent;
        public long Bytes { get; set; }
        public int FileCount { get; set; }
        public int DirCount { get; set; }
    }

    // ---- names ----

    private static string DirName(Random rng)
    {
        var word = Words[rng.Next(Words.Length)];
        return rng.Next(4) switch
        {
            0 => word + "-" + Words[rng.Next(Words.Length)],
            1 => word + rng.Next(1, 40).ToString(System.Globalization.CultureInfo.InvariantCulture),
            _ => word,
        };
    }

    private static string FileName(Random rng)
    {
        var roll = rng.NextDouble();
        if (roll < 0.02) return $"IMG_{rng.Next(0, 10000):0000}.jpg";
        if (roll < 0.023) return "report" + Extension(rng);
        if (roll < 0.028) return $"{Words[rng.Next(Words.Length)]}-report-{rng.Next(2019, 2027)}{Extension(rng)}";

        var name = Words[rng.Next(Words.Length)];
        switch (rng.Next(5))
        {
            case 0: name += "-" + Words[rng.Next(Words.Length)]; break;
            case 1: name += "_" + rng.Next(0, 10000).ToString("0000", System.Globalization.CultureInfo.InvariantCulture); break;
            case 2: name = char.ToUpperInvariant(name[0]) + name[1..] + Words[rng.Next(Words.Length)]; break;
        }

        return name + Extension(rng);
    }

    private static string Extension(Random rng)
    {
        var roll = rng.NextDouble() * ExtensionWeightTotal;
        foreach (var (ext, weight) in Extensions)
        {
            roll -= weight;
            if (roll <= 0) return ext;
        }

        return Extensions[^1].Ext;
    }

    private static long FileSize(Random rng)
    {
        if (rng.NextDouble() < 0.006) return rng.NextInt64(100L * 1024 * 1024, 1024L * 1024 * 1024);

        // Log-normal, median 6 KB, long right tail. Capped below the 100 GB the worst-case query asks for.
        var gaussian = Math.Sqrt(-2.0 * Math.Log(1.0 - rng.NextDouble())) * Math.Cos(2.0 * Math.PI * rng.NextDouble());
        var size = Math.Exp(8.72 + 2.6 * gaussian);
        return (long)Math.Clamp(size, 0, 50L * 1024 * 1024 * 1024);
    }

    private static DateTime Modified(Random rng)
    {
        if (rng.NextDouble() < 0.01) return Today.AddSeconds(rng.Next(0, 86_400));
        var span = DateCeiling - DateFloor;
        return DateFloor.AddSeconds(rng.NextDouble() * span.TotalSeconds);
    }

    // ---- the world ----

    private static readonly (string Relative, bool Hidden)[] Skeleton =
    [
        ("Users", false), (@"Users\bench", false), (@"Users\bench\Documents", false),
        (@"Users\bench\Pictures", false), (@"Users\bench\Videos", false), (@"Users\bench\Downloads", false),
        (@"Users\bench\Desktop", false), (@"Users\bench\Music", false),
        (@"Users\bench\AppData", true), (@"Users\bench\AppData\Local", false), (@"Users\bench\AppData\Roaming", false),
        (@"Users\bench\AppData\Local\Temp", false), (@"Users\bench\AppData\Local\Packages", false),
        (@"Users\Public", false), (@"Users\Default", true),
        ("Source", false),
        (@"Source\bertbrowser", false), (@"Source\bertbrowser\src", false), (@"Source\bertbrowser\tests", false),
        (@"Source\ledger", false), (@"Source\ledger\src", false), (@"Source\ledger\node_modules", false),
        (@"Source\website", false), (@"Source\website\node_modules", false), (@"Source\website\public", false),
        (@"Source\kernel-tools", false), (@"Source\kernel-tools\build", false),
        (@"Source\photoshelf", false), (@"Source\photoshelf\bin", false),
        (@"Source\infra", false), (@"Source\infra\terraform", false),
        (@"Source\notes", false), (@"Source\scratch", false), (@"Source\playground", false),
        (@"Source\vendor", false), (@"Source\vendor\sdk", false),
        (@"Source\archive", false), (@"Source\archive\2019", false), (@"Source\archive\2021", false),
        ("Windows", false), (@"Windows\System32", false), (@"Windows\System32\drivers", false),
        (@"Windows\System32\DriverStore", false), (@"Windows\SysWOW64", false), (@"Windows\WinSxS", false),
        (@"Windows\WinSxS\Manifests", false), (@"Windows\Temp", false), (@"Windows\assembly", false),
        (@"Windows\Fonts", false), (@"Windows\Logs", false), (@"Windows\servicing", false),
        ("Program Files", false),
        (@"Program Files\Common Files", false), (@"Program Files\dotnet", false), (@"Program Files\dotnet\shared", false),
        (@"Program Files\Git", false), (@"Program Files\Git\mingw64", false), (@"Program Files\7-Zip", false),
        (@"Program Files\Microsoft VS Code", false), (@"Program Files\Microsoft VS Code\resources", false),
        (@"Program Files\Mozilla Firefox", false), (@"Program Files\Google", false), (@"Program Files\Google\Chrome", false),
        (@"Program Files\Microsoft Office", false), (@"Program Files\Microsoft Office\root", false),
        (@"Program Files\Windows Defender", false), (@"Program Files\WindowsApps", true),
        (@"Program Files\Adobe", false), (@"Program Files\Blender Foundation", false), (@"Program Files\Docker", false),
        (@"Program Files\Java", false), (@"Program Files\Python312", false), (@"Program Files\Python312\Lib", false),
        (@"Program Files\NVIDIA Corporation", false), (@"Program Files\Steam", false), (@"Program Files\Steam\steamapps", false),
        (@"Program Files\OBS Studio", false), (@"Program Files\VideoLAN", false), (@"Program Files\PowerShell", false),
        (@"Program Files\Unity", false), (@"Program Files\Unity\Editor", false), (@"Program Files\JetBrains", false),
        (@"Program Files\Notepad++", false), (@"Program Files\PuTTY", false), (@"Program Files\WinSCP", false),
        ("Program Files (x86)", false),
        (@"Program Files (x86)\Common Files", false), (@"Program Files (x86)\Microsoft SDKs", false),
        (@"Program Files (x86)\Windows Kits", false), (@"Program Files (x86)\Windows Kits\10", false),
        (@"Program Files (x86)\Steam", false), (@"Program Files (x86)\Internet Explorer", false),
        (@"Program Files (x86)\Microsoft", false), (@"Program Files (x86)\Reference Assemblies", false),
        (@"Program Files (x86)\MSBuild", false), (@"Program Files (x86)\Intel", false),
        ("ProgramData", true), (@"ProgramData\Microsoft", false), (@"ProgramData\Package Cache", false),
        (@"ProgramData\Docker", false), (@"ProgramData\chocolatey", false),
        ("$Recycle.Bin", true), ("Recovery", true), ("PerfLogs", false), ("inetpub", false),
    ];

    private static readonly (string Ext, double Weight)[] Extensions =
    [
        (".dll", 14), (".txt", 9), (".cs", 8), (".json", 7), (".xml", 6), (".png", 6), (".jpg", 6),
        (".h", 4), (".cpp", 4), (".js", 4), (".ts", 3), (".md", 3), (".log", 3), (".exe", 2),
        (".mp4", 1.5), (".pdf", 1.5), (".zip", 1), (".manifest", 1.5), (".mui", 1.5), (".cat", 1),
        (".pri", 1), (".dat", 1), (".svg", 0.5), (".css", 0.5), (".html", 0.5), (".yml", 0.5),
        (".docx", 0.3), (".xlsx", 0.3), (".mp3", 0.3), (".gif", 0.3), (".ico", 0.3), (".nupkg", 0.3),
        (".pdb", 1), (".lib", 0.5), (".obj", 0.5), (".resx", 0.3), (".xaml", 0.3), (".ps1", 0.2),
    ];

    private static readonly double ExtensionWeightTotal = Extensions.Sum(e => e.Weight);

    private static readonly string[] Words =
    [
        "alpha", "anchor", "annual", "api", "app", "archive", "asset", "audio", "backup", "banner", "base",
        "batch", "bench", "binary", "block", "board", "bridge", "buffer", "build", "bundle", "cache", "camera",
        "canvas", "card", "catalog", "chain", "channel", "chart", "client", "cloud", "cluster", "code", "color",
        "common", "compat", "config", "console", "content", "control", "copy", "core", "crypto", "cursor",
        "daemon", "data", "debug", "default", "demo", "design", "desktop", "device", "dialog", "diff", "digest",
        "disk", "display", "doc", "draft", "driver", "editor", "engine", "entry", "error", "event", "export",
        "extension", "feature", "field", "file", "filter", "final", "folder", "font", "form", "frame", "game",
        "gateway", "graph", "grid", "group", "guide", "handler", "header", "helper", "history", "home", "host",
        "icon", "image", "import", "index", "info", "input", "install", "invoice", "item", "job", "journal",
        "kernel", "key", "label", "layer", "layout", "ledger", "legacy", "library", "license", "link", "list",
        "loader", "local", "log", "machine", "main", "manifest", "map", "media", "memo", "menu", "merge",
        "message", "meta", "model", "module", "monitor", "mount", "native", "network", "node", "notes",
        "object", "offline", "output", "package", "page", "panel", "parser", "patch", "path", "photo", "pipeline",
        "plan", "player", "plugin", "policy", "pool", "preview", "profile", "project", "proxy", "query", "queue",
        "readme", "record", "registry", "release", "render", "resource", "result", "review", "root", "route",
        "runtime", "sample", "scan", "schema", "screen", "script", "search", "secret", "segment", "server",
        "service", "session", "settings", "setup", "shader", "shared", "shell", "signal", "sketch", "slide",
        "snapshot", "socket", "source", "spec", "stack", "stage", "state", "status", "storage", "store", "stream",
        "string", "style", "summary", "support", "sync", "system", "table", "target", "task", "template", "temp",
        "test", "texture", "theme", "thread", "thumbnail", "ticket", "timeline", "token", "tool", "trace",
        "track", "transfer", "tree", "update", "upload", "user", "util", "value", "vendor", "version", "video",
        "view", "volume", "wallet", "web", "widget", "window", "worker", "workspace", "zone",
    ];
}
