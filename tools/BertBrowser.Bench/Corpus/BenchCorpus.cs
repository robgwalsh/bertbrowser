using System.Globalization;
using System.Text;
using BertBrowser.Core.Data;
using Microsoft.Data.Sqlite;

namespace BertBrowser.Bench.Corpus;

/// <summary>Where the synthetic disk tree lives, once built.</summary>
/// <param name="Flat">One folder of 10,000 × scale empty files, every fourth a <c>.txt</c>: a listing benchmark.</param>
/// <param name="Startup">500 mixed files: what the first tab opens on in a Tier C launch.</param>
/// <param name="Deep">20,000 × scale small files in 2,000 × scale folders, up to eight deep: the walkers' tree.</param>
/// <param name="Text">2,000 × scale text files of 2–64 KB, a tenth of them carrying the needle
/// <see cref="Needle"/> and a tenth binary: the content search's tree.</param>
/// <param name="Hash">1 MB files for the hasher and the duplicate finder: 32 unique, 25 identical pairs,
/// and 100 that are identical for their first 64 KB and differ after — the prefix-then-full worst case.</param>
/// <param name="Big">A 64 MB file of random bytes.</param>
/// <param name="BigCopy">The same bytes again, for the content comparison.</param>
internal sealed record TreePaths(string Root, string Flat, string Startup, string Deep, string Text, string Hash, string Big, string BigCopy);

/// <summary>
/// Builds the synthetic corpus once and caches it under <c>%LOCALAPPDATA%\bertbrowser-bench</c>.
/// </summary>
/// <remarks>
/// Every cache path carries <see cref="SyntheticPaths.GeneratorVersion"/> (and the tree its own
/// <see cref="TreeLayoutVersion"/>), so changing a generator invalidates every cached copy rather than
/// silently comparing a new world against an old baseline. A <c>.ready</c> marker is written last;
/// without it the artifact is rebuilt, so an interrupted build never leaves a half-corpus that looks
/// whole.
/// </remarks>
internal static class BenchCorpus
{
    /// <summary>Bump when the tree's contents change shape; the database has the generator's version.</summary>
    public const int TreeLayoutVersion = 2;

    /// <summary>The word a tenth of the <c>Text</c> files contain, and the content query looks for.</summary>
    public const string Needle = "benchneedle";

    private const int UpsertChunk = 20_000;
    private const string Ready = ".ready";
    private const int Megabyte = 1024 * 1024;

    public static string DefaultRoot =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "bertbrowser-bench");

    public static string DbPath(string root, int scale) =>
        Path.Combine(root, $"corpus-v{SyntheticPaths.GeneratorVersion}-s{scale}.db");

    public static string TreeRoot(string root, int scale) =>
        Path.Combine(root, $"tree-v{SyntheticPaths.GeneratorVersion}.{TreeLayoutVersion}-s{scale}");

    public static string ArtifactsPath(string root) => Path.Combine(root, "artifacts");

    public static TreePaths TreePaths(string root, int scale)
    {
        var tree = TreeRoot(root, scale);
        return new TreePaths(
            tree,
            Path.Combine(tree, "Flat"),
            Path.Combine(tree, "Startup"),
            Path.Combine(tree, "Deep"),
            Path.Combine(tree, "Text"),
            Path.Combine(tree, "Hash"),
            Path.Combine(tree, "Big64MB.bin"),
            Path.Combine(tree, "Big64MB-copy.bin"));
    }

    public static void EnsureAll(string root, int scale, Action<string> log, bool rebuild = false)
    {
        EnsureDb(root, scale, log, rebuild);
        EnsureTree(root, scale, log, rebuild);
    }

    /// <summary>The index database: <c>fs_entry</c> and <c>dir_size_cache</c> for the whole invented disk.</summary>
    public static string EnsureDb(string root, int scale, Action<string> log, bool rebuild = false)
    {
        var path = DbPath(root, scale);
        var marker = path + Ready;
        if (!rebuild && File.Exists(marker) && File.Exists(path)) return path;

        Directory.CreateDirectory(root);
        DeleteDbFiles(path);
        File.Delete(marker);

        var clock = System.Diagnostics.Stopwatch.StartNew();
        log($"# corpus: generating {scale * 100_000:N0} rows (scale {scale})");
        var model = SyntheticPaths.Generate(scale);

        var db = new Db(path);
        db.Migrate();
        var index = new FsIndexRepository(db);
        var sizes = new DirSizeRepository(db);

        foreach (var chunk in model.Rows.Chunk(UpsertChunk))
            index.UpsertEntries(chunk, crawlGen: 1);
        index.UpsertRoot(Core.Paths.PathKey.Canonicalize(SyntheticPaths.Drive), SyntheticPaths.Drive, model.DirectorySizes[0].ComputedUtc, complete: true);
        sizes.UpsertMany(model.DirectorySizes);

        Checkpoint(db);
        SqliteConnection.ClearAllPools();

        File.WriteAllText(marker, DateTime.UtcNow.ToString("O", CultureInfo.InvariantCulture));
        log($"# corpus: {path} ({new FileInfo(path).Length / Megabyte} MB) in {clock.Elapsed.TotalSeconds:0.0} s");
        return path;
    }

    /// <summary>The on-disk tree for benchmarks that read a real file system.</summary>
    public static TreePaths EnsureTree(string root, int scale, Action<string> log, bool rebuild = false)
    {
        var paths = TreePaths(root, scale);
        var marker = Path.Combine(paths.Root, Ready);
        if (!rebuild && File.Exists(marker)) return paths;

        if (Directory.Exists(paths.Root)) Directory.Delete(paths.Root, recursive: true);
        Directory.CreateDirectory(paths.Root);

        var clock = System.Diagnostics.Stopwatch.StartNew();
        log($"# corpus: writing tree under {paths.Root} (a few hundred MB; once)");

        var rng = new Random(777 + scale);
        WriteFlat(paths.Flat, 10_000 * scale);
        WriteFlat(paths.Startup, 500);
        WriteDeep(paths.Deep, 20_000 * scale, 2_000 * scale, rng);
        WriteText(paths.Text, 2_000 * scale, rng);
        WriteHash(paths.Hash, rng);
        WriteRandom(paths.Big, 64 * Megabyte, rng);
        File.Copy(paths.Big, paths.BigCopy, overwrite: true);

        File.WriteAllText(marker, DateTime.UtcNow.ToString("O", CultureInfo.InvariantCulture));
        log($"# corpus: tree ready in {clock.Elapsed.TotalSeconds:0.0} s");
        return paths;
    }

    /// <summary>What is in a built corpus, for <c>corpus --stats</c>.</summary>
    public static string Describe(string root, int scale)
    {
        var dbPath = DbPath(root, scale);
        if (!File.Exists(dbPath)) return "not built";

        var db = new Db(dbPath, create: false);
        using var conn = db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "SELECT COUNT(*), SUM(is_dir), SUM(size_bytes) FROM fs_entry;";
        using var reader = cmd.ExecuteReader();
        reader.Read();
        var rows = reader.GetInt64(0);
        var dirs = reader.GetInt64(1);
        var bytes = reader.GetInt64(2);

        var tree = TreeRoot(root, scale);
        var treeBytes = Directory.Exists(tree)
            ? new DirectoryInfo(tree).EnumerateFiles("*", SearchOption.AllDirectories).Sum(f => f.Length)
            : 0;

        return $"{rows:N0} rows ({dirs:N0} directories), {bytes / (1024.0 * Megabyte):0.0} GB of invented files, " +
               $"{new FileInfo(dbPath).Length / Megabyte} MB database; {treeBytes / Megabyte} MB of real files in the tree";
    }

    // ---- the tree ----

    private static readonly string[] ExtensionCycle = [".jpg", ".png", ".mp4", ".txt", ".cs", ".json", ".xml", ".txt"];

    private static void WriteFlat(string dir, int count)
    {
        Directory.CreateDirectory(dir);
        for (var i = 0; i < count; i++)
        {
            using var _ = File.Create(Path.Combine(dir, $"f{i:00000}{ExtensionCycle[i % ExtensionCycle.Length]}"));
        }
    }

    /// <summary>A random tree of <paramref name="dirs"/> folders no deeper than eight, with the files
    /// spread across them and each holding 0–4 KB so a walk reads real sizes.</summary>
    private static void WriteDeep(string root, int files, int dirs, Random rng)
    {
        Directory.CreateDirectory(root);
        var folders = new List<(string Path, int Depth)> { (root, 0) };
        for (var d = 0; d < dirs; d++)
        {
            var parent = folders[rng.Next(folders.Count)];
            if (parent.Depth >= 8) { d--; continue; }
            var path = Path.Combine(parent.Path, $"d{d:0000}");
            Directory.CreateDirectory(path);
            folders.Add((path, parent.Depth + 1));
        }

        var buffer = new byte[4096];
        rng.NextBytes(buffer);
        for (var i = 0; i < files; i++)
        {
            var folder = folders[1 + i % (folders.Count - 1)].Path;
            var size = rng.Next(0, 4097);
            using var file = File.Create(Path.Combine(folder, $"f{i:00000}{ExtensionCycle[i % ExtensionCycle.Length]}"));
            file.Write(buffer, 0, size);
        }
    }

    /// <summary>Word salad, UTF-8, 2–64 KB; one in ten carries the needle, one in ten is binary.</summary>
    private static void WriteText(string dir, int count, Random rng)
    {
        Directory.CreateDirectory(dir);
        string[] words = ["the", "index", "reads", "every", "name", "once", "and", "patches", "it", "from", "the",
                          "journal", "so", "a", "search", "costs", "a", "range", "scan", "never", "a", "walk"];
        var sb = new StringBuilder();
        for (var i = 0; i < count; i++)
        {
            var size = rng.Next(2 * 1024, 64 * 1024);
            var path = Path.Combine(dir, $"t{i:00000}.txt");

            if (i % 10 == 9)
            {
                var bytes = new byte[size];
                rng.NextBytes(bytes);
                File.WriteAllBytes(path, bytes);
                continue;
            }

            sb.Clear();
            var needleAt = i % 10 == 0 ? rng.Next(size / 4, size / 2) : -1;
            while (sb.Length < size)
            {
                if (needleAt >= 0 && sb.Length >= needleAt) { sb.Append(Needle).Append(' '); needleAt = -1; }
                sb.Append(words[rng.Next(words.Length)]).Append(rng.Next(12) == 0 ? ".\n" : " ");
            }

            File.WriteAllText(path, sb.ToString(), Encoding.UTF8);
        }
    }

    private static void WriteHash(string dir, Random rng)
    {
        Directory.CreateDirectory(dir);
        var buffer = new byte[Megabyte];

        for (var i = 0; i < 32; i++)
        {
            rng.NextBytes(buffer);
            File.WriteAllBytes(Path.Combine(dir, $"unique{i:00}.bin"), buffer);
        }

        for (var i = 0; i < 25; i++)
        {
            rng.NextBytes(buffer);
            File.WriteAllBytes(Path.Combine(dir, $"pair{i:00}-a.bin"), buffer);
            File.WriteAllBytes(Path.Combine(dir, $"pair{i:00}-b.bin"), buffer);
        }

        // The same first 64 KB on every one, then different bytes: the prefix pass groups them all
        // and the full pass has to read every byte to tell them apart.
        var prefix = new byte[64 * 1024];
        rng.NextBytes(prefix);
        for (var i = 0; i < 100; i++)
        {
            rng.NextBytes(buffer);
            prefix.CopyTo(buffer, 0);
            File.WriteAllBytes(Path.Combine(dir, $"nearmiss{i:000}.bin"), buffer);
        }
    }

    private static void WriteRandom(string path, int bytes, Random rng)
    {
        var buffer = new byte[Megabyte];
        using var file = File.Create(path);
        for (var written = 0; written < bytes; written += buffer.Length)
        {
            rng.NextBytes(buffer);
            file.Write(buffer, 0, Math.Min(buffer.Length, bytes - written));
        }
    }

    // ---- the database ----

    private static void Checkpoint(Db db)
    {
        using var conn = db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "PRAGMA wal_checkpoint(TRUNCATE);";
        cmd.ExecuteNonQuery();
    }

    private static void DeleteDbFiles(string path)
    {
        SqliteConnection.ClearAllPools();
        foreach (var suffix in new[] { "", "-wal", "-shm" })
        {
            if (File.Exists(path + suffix)) File.Delete(path + suffix);
        }
    }
}
