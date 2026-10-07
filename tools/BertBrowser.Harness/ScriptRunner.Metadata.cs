using System.IO;
using System.Windows;
using BertBrowser.App.ViewModels;
using BertBrowser.App.Views;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Metadata;

namespace BertBrowser.Harness;

/// <summary>
/// The metadata pane's verbs: real pictures and songs to edit, switching the side pane, typing
/// into the pane's boxes, and reading a file's tags back.
/// </summary>
/// <remarks>
/// <b>Every write here goes through the active tab's own pane</b> — its boxes, its Apply, and so
/// the shell's queue and undo history — never straight to the executor. A script that edited files
/// by a shorter path would prove the shorter path.
/// </remarks>
internal sealed partial class ScriptRunner
{
    /// <summary>
    /// <c>metadata-fixture [folder]</c> — a camera picture, a bare picture and two songs from one
    /// album, all from <c>MetadataFixtures</c>, the same bytes the unit tests edit.
    /// </summary>
    private void MetadataFixture(string rest)
    {
        var root = _sandbox.RequireInside(rest.Length == 0 ? "." : rest, "metadata-fixture");
        Directory.CreateDirectory(root);

        File.WriteAllBytes(Path.Combine(root, "camera.jpg"), MetadataFixtures.Jpeg(MetadataFixtures.CameraJpeg));
        File.WriteAllBytes(Path.Combine(root, "plain.jpg"), MetadataFixtures.Jpeg(MetadataFixtures.PlainJpeg));

        File.WriteAllBytes(Path.Combine(root, "first.mp3"), MetadataFixtures.Mp3(
            [("TIT2", "First song"), ("TPE1", "The Band"), ("TALB", "Album One"), ("TRCK", "1")]));
        File.WriteAllBytes(Path.Combine(root, "second.mp3"), MetadataFixtures.Mp3(
            [("TIT2", "Second song"), ("TPE1", "The Band"), ("TALB", "Album One"), ("TRCK", "2")], withV1: true));

        Console.WriteLine($"# metadata fixture: {root}");
    }

    /// <summary>
    /// <c>media-fixture [folder]</c> — the same short tone as FLAC, WAV, M4A and WMA, and a
    /// two-track MP4 video, all untagged and all encoded by Windows rather than by this app.
    /// </summary>
    private void MediaFixture(string rest)
    {
        var root = _sandbox.RequireInside(rest.Length == 0 ? "." : rest, "media-fixture");
        Directory.CreateDirectory(root);

        File.WriteAllBytes(Path.Combine(root, "tone.flac"), MetadataFixtures.Bytes(MetadataFixtures.Flac));
        File.WriteAllBytes(Path.Combine(root, "tone.m4a"), MetadataFixtures.Bytes(MetadataFixtures.M4a));
        File.WriteAllBytes(Path.Combine(root, "tone.wma"), MetadataFixtures.Bytes(MetadataFixtures.Wma));
        File.WriteAllBytes(Path.Combine(root, "tone.wav"), MetadataFixtures.Wav());
        File.WriteAllBytes(Path.Combine(root, "clip.mp4"), MetadataFixtures.Bytes(MetadataFixtures.Mp4Video));

        Console.WriteLine($"# media fixture: {root}");
    }

    /// <summary>
    /// <c>formats-fixture [folder]</c> — one small file of each further kind the pane edits: a
    /// PNG, a WebP, a TIFF, a Word document, an EPUB book, and the tone as AIFF and as Opus.
    /// </summary>
    private void FormatsFixture(string rest)
    {
        var root = _sandbox.RequireInside(rest.Length == 0 ? "." : rest, "formats-fixture");
        Directory.CreateDirectory(root);

        File.WriteAllBytes(Path.Combine(root, "pixel.png"), Convert.FromBase64String(MetadataFixtures.Png));
        File.WriteAllBytes(Path.Combine(root, "pixel.webp"), Convert.FromBase64String(MetadataFixtures.WebP));
        File.WriteAllBytes(Path.Combine(root, "report.docx"), MetadataFixtures.Docx());
        File.WriteAllBytes(Path.Combine(root, "book.epub"), MetadataFixtures.Epub());
        File.WriteAllBytes(Path.Combine(root, "scan.tif"), Convert.FromBase64String(MetadataFixtures.Tiff));
        File.WriteAllBytes(Path.Combine(root, "tone.aiff"), MetadataFixtures.Aiff());
        File.WriteAllBytes(Path.Combine(root, "tone.opus"), MetadataFixtures.Opus());

        Console.WriteLine($"# formats fixture: {root}");
    }

    /// <summary>
    /// <c>windows-fixture [folder]</c> — files no codec here writes: the tone as <c>.asf</c>,
    /// which Windows' own in-box handler does write, and a GIF, whose handler writes nothing.
    /// </summary>
    private void WindowsFixture(string rest)
    {
        var root = _sandbox.RequireInside(rest.Length == 0 ? "." : rest, "windows-fixture");
        Directory.CreateDirectory(root);

        File.WriteAllBytes(Path.Combine(root, "tone.asf"), MetadataFixtures.Bytes(MetadataFixtures.Wma));
        File.WriteAllBytes(Path.Combine(root, "dot.gif"),
            Convert.FromBase64String("R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw=="));

        Console.WriteLine($"# windows fixture: {root}");
    }

    /// <summary><c>metadata-picture cover.jpg</c> — embeds a picture file from the folder being
    /// shown as the cover of the selection, as choosing it in the picker would.</summary>
    private void MetadataPicture(string rest)
    {
        var image = File.ReadAllBytes(Row(Require(rest, "metadata-picture")).FullPath);
        var vm = MetadataPaneFor("");

        Await(() => vm.SetPictureAsync(image));
        Console.WriteLine($"# metadata: {session.Dispatcher.Invoke(() => vm.Summary)}");
    }

    /// <summary>
    /// <c>side-pane none|preview|metadata</c> sets what stands beside the list;
    /// <c>side-pane cycle</c> runs the cycle command a key can be bound to.
    /// </summary>
    private void SidePaneVerb(string rest)
    {
        Invoke(() =>
        {
            switch (rest.Trim().ToLowerInvariant())
            {
                case "none": session.Tab.SidePane = SidePane.None; break;
                case "preview": session.Tab.SidePane = SidePane.Preview; break;
                case "metadata": session.Tab.SidePane = SidePane.Metadata; break;
                case "cycle": session.Tab.CycleSidePaneCommand.Execute(null); break;
                default: throw new FormatException("side-pane wants none, preview, metadata or cycle.");
            }
        });
        session.Settle(quietMs: 400);
    }

    /// <summary><c>assert-side-pane metadata</c> — which pane the active tab is showing.</summary>
    private void AssertSidePane(string rest)
    {
        var actual = session.Dispatcher.Invoke(() => session.Tab.SidePane.ToString());
        if (!actual.Equals(rest.Trim(), StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"the side pane is {actual}, expected {rest}.");
    }

    /// <summary>
    /// <c>metadata-type title=New title; album=</c> — types into the pane's boxes for the
    /// selection without applying, so a <c>shot</c> can photograph it mid-edit.
    /// </summary>
    private void MetadataType(string rest)
    {
        MetadataPaneFor(rest);

        // The boxes have the text at once; their outlines, notes and the Apply button follow on
        // the dispatcher, and a shot taken before that photographs the pane as it was.
        session.Settle(quietMs: 200);
    }

    /// <summary>
    /// <c>metadata-set title=New title; rating=4; modified=2020-01-02 03:04:05</c> — types into
    /// the pane's boxes for the selection and presses Apply. An empty value clears the field.
    /// </summary>
    private void MetadataSet(string rest)
    {
        if (rest.Length == 0) throw new FormatException("metadata-set wants 'field=value; field=value'.");

        var vm = MetadataPaneFor(rest);
        Press(vm.ApplyCommand, vm, $"apply '{rest}'");
    }

    /// <summary><c>metadata-remove location</c> or <c>metadata-remove all</c> — the pane's two
    /// Remove buttons, pressed for the selection.</summary>
    private void MetadataRemove(string rest)
    {
        var vm = MetadataPaneFor("");
        Press(rest.Trim().ToLowerInvariant() switch
        {
            "location" => vm.RemoveLocationCommand,
            "all" => vm.RemoveAllCommand,
            "picture" => vm.RemovePictureCommand,
            _ => throw new FormatException("metadata-remove wants 'location', 'picture' or 'all'."),
        }, vm, $"remove {rest}");
    }

    /// <summary><c>assert-file-date camera.jpg modified 2021-06-05 04:03:02</c> — a file's own
    /// date on disk, in local time.</summary>
    private void AssertFileDate(string rest)
    {
        var (name, tail) = Split(rest);
        var (which, expected) = Split(tail);
        var path = Row(name).FullPath;

        var actual = (which.ToLowerInvariant() switch
        {
            "modified" => Directory.Exists(path) ? Directory.GetLastWriteTime(path) : File.GetLastWriteTime(path),
            "created" => Directory.Exists(path) ? Directory.GetCreationTime(path) : File.GetCreationTime(path),
            _ => throw new FormatException("assert-file-date wants 'modified' or 'created'."),
        }).ToString(MetadataFields.DateFormat, System.Globalization.CultureInfo.InvariantCulture);

        if (actual != expected.Trim('"'))
            throw new AssertionException($"'{name}' was {which} {actual}, expected {expected}.");
    }

    private void Press(CommunityToolkit.Mvvm.Input.IAsyncRelayCommand command, MetadataViewModel vm, string what)
    {
        if (!session.Dispatcher.Invoke(() => command.CanExecute(null)))
            throw new AssertionException($"the metadata pane would not {what}: {session.Dispatcher.Invoke(() => vm.Summary)}");

        Await(() => command.ExecuteAsync(null));
        Console.WriteLine($"# metadata: {session.Dispatcher.Invoke(() => vm.Summary)}");
    }

    /// <summary>
    /// <c>assert-tag camera.jpg title New title</c> — what the file holds now, read from disk
    /// through the codec. <c>(none)</c> for a field that should be absent.
    /// </summary>
    private void AssertTag(string rest)
    {
        var (name, tail) = Split(rest);
        var (id, expected) = Split(tail);
        if (expected.Length == 0) throw new FormatException("assert-tag wants a row, a field and the value, or (none).");

        var path = Row(name).FullPath;
        MetadataDocument document;
        if (MetadataCodecs.For(path) is { } codec)
        {
            using var stream = ReadOnlyFile.TryOpen(path)
                               ?? throw new AssertionException($"'{name}' could not be opened to read its tags.");
            document = codec.Read(stream);
        }
        else
        {
            // No codec: what the pane itself would read, through Windows' property handler.
            document = new BertBrowser.App.Services.WindowsPropertyFallback().Read(path)?.Document
                       ?? throw new AssertionException($"nothing reads the metadata of '{name}'.");
        }

        // Not a field — it is never typed, only removed — but asserted the same way: yes or (none).
        string actual, label;
        if (id.Equals("location", StringComparison.OrdinalIgnoreCase))
        {
            (actual, label) = (document.HasLocation ? "yes" : "", "location");
        }
        else if (id.Equals("picture", StringComparison.OrdinalIgnoreCase))
        {
            (actual, label) = (document.Picture is null ? "" : "yes", "cover picture");
        }
        else
        {
            var spec = MetadataFields.Find(id)
                       ?? throw new FormatException($"'{id}' is not a metadata field. Try: location, {string.Join(", ", MetadataFields.All.Select(f => f.Id))}.");
            (actual, label) = (document.Get(spec.Field), spec.Label);
        }

        var wanted = expected == "(none)" ? "" : expected.Trim('"');
        if (!string.Equals(actual, wanted, StringComparison.Ordinal))
            throw new AssertionException($"the {label} of '{name}' is '{actual}', expected '{wanted}'.");
    }

    /// <summary><c>assert-metadata-file notes.txt Skipped only its dates</c> — an item's state in
    /// the pane, and optionally words its reason contains.</summary>
    private void AssertMetadataFile(string rest)
    {
        var (name, tail) = Split(rest);
        var (state, words) = Split(tail);
        var vm = MetadataPaneFor("");

        var (actual, detail) = session.Dispatcher.Invoke(() =>
        {
            var row = vm.Files.FirstOrDefault(f => string.Equals(f.Name, name, StringComparison.OrdinalIgnoreCase))
                      ?? throw new AssertionException($"'{name}' is not in the pane. It lists: {string.Join(", ", vm.Files.Select(f => f.Name))}");
            return (row.State.ToString(), row.Detail);
        });

        if (!string.Equals(actual, state, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"'{name}' is {actual} in the pane ({detail}), expected {state}.");
        if (words.Length > 0 && !detail.Contains(words, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"'{name}' says '{detail}', expected it to mention '{words}'.");
    }

    /// <summary><c>assert-metadata-detail Dimensions 8 x 8</c> — a row of the pane's read-only
    /// details, by words its label and its value contain.</summary>
    private void AssertMetadataDetail(string rest)
    {
        var (label, value) = Split(rest);
        var vm = MetadataPaneFor("");

        var rows = session.Dispatcher.Invoke(() => vm.Details.Select(d => $"{d.Label}: {d.Value}").ToList());
        if (!rows.Any(r => r.StartsWith(label, StringComparison.OrdinalIgnoreCase) &&
                           r.Contains(value, StringComparison.OrdinalIgnoreCase)))
            throw new AssertionException($"no detail '{label}' containing '{value}'. The pane shows: {string.Join(" | ", rows)}");
    }

    /// <summary><c>assert-metadata-field album (multiple)</c>, or a value, <c>(none)</c>, or
    /// <c>(absent)</c> for a box the selection does not have.</summary>
    private void AssertMetadataField(string rest)
    {
        var (id, expected) = Split(rest);
        var vm = MetadataPaneFor("");

        var actual = session.Dispatcher.Invoke(() =>
        {
            var row = vm.Fields.FirstOrDefault(f => string.Equals(f.Id, id, StringComparison.OrdinalIgnoreCase));
            if (row is null) return "(absent)";
            if (row.ShowsMixedPlaceholder) return "(multiple)";
            return row.Text.Length == 0 ? "(none)" : row.Text;
        });

        if (!string.Equals(actual, expected.Trim('"'), StringComparison.Ordinal))
            throw new AssertionException($"the pane's '{id}' box shows {actual}, expected {expected}.");
    }

    /// <summary>
    /// The active tab's metadata pane, opened if it was not, showing the selection, with
    /// <paramref name="typed"/> typed into its boxes.
    /// </summary>
    private MetadataViewModel MetadataPaneFor(string typed)
    {
        Invoke(() => session.Tab.SidePane = SidePane.Metadata);
        session.Settle();

        var vm = session.Dispatcher.Invoke(() =>
        {
            var view = FindNamed<FrameworkElement>("FileListView");
            var tabView = VisualTreeUtil.FindAncestor<DirectoryTabView>(view)
                          ?? throw new AssertionException("The file list is not inside a DirectoryTabView.");

            // The pane follows the selection on a posted callback; ask now rather than wait for it.
            tabView.ShowMetadataForSelection();
            return tabView.Metadata ?? throw new AssertionException("The metadata pane did not open.");
        });

        // Await, not Invoke: the read's continuation wants the dispatcher this thread would be blocking.
        // Twice, with a quiet spell between: a selection change posts its own reload at background
        // priority, and typing into boxes that reload is about to replace would be typing into nothing.
        Await(() => vm.Loaded);
        session.Settle(quietMs: 200);
        Await(() => vm.Loaded);

        // A piece with no '=' belongs to the value before it: a list field's own values are
        // separated by semicolons too, and "keywords=sea; boats" should mean what it says.
        var pairs = new List<(string Id, string Value)>();
        foreach (var part in typed.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        {
            var equals = part.IndexOf('=');
            if (equals > 0) pairs.Add((part[..equals].Trim(), part[(equals + 1)..].Trim()));
            else if (pairs.Count > 0) pairs[^1] = (pairs[^1].Id, pairs[^1].Value + "; " + part);
            else throw new FormatException($"'{part}' is not 'field=value'.");
        }

        foreach (var (id, value) in pairs)
        {
            session.Dispatcher.Invoke(() =>
            {
                var row = vm.Fields.FirstOrDefault(f => string.Equals(f.Id, id, StringComparison.OrdinalIgnoreCase))
                          ?? throw new AssertionException(
                              $"the pane has no '{id}' box for this selection. It has: {string.Join(", ", vm.Fields.Select(f => f.Id))}");

                // Through a different value first, so emptying a box that started empty still
                // counts as typing in it — which is how a person clears a mixed field.
                row.Text = value + " ";
                row.Text = value;
            });
        }

        return vm;
    }
}
