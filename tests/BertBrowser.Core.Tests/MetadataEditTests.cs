using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Metadata;
using BertBrowser.Core.Services.Rename;
using BertBrowser.Core.Services.Transfer;
using BertBrowser.Core.Services.UndoHistory;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The planner against a fake disk, and the executor and its undo record against a real one.
/// </summary>
public sealed class MetadataEditTests : IDisposable
{
    private readonly string _root;
    private readonly MetadataEditPlanner _planner = new();
    private readonly MetadataEditExecutor _executor = new();

    public MetadataEditTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-metadata-{Guid.NewGuid():N}");
        Directory.CreateDirectory(_root);
    }

    public void Dispose()
    {
        try
        {
            foreach (var file in Directory.EnumerateFiles(_root, "*", SearchOption.AllDirectories))
                File.SetAttributes(file, FileAttributes.Normal);
            Directory.Delete(_root, recursive: true);
        }
        catch (IOException)
        {
        }
    }

    private string P(params string[] parts) => Path.Combine([_root, .. parts]);

    private string Jpeg(string name, string fixture = MetadataFixtures.CameraJpeg)
    {
        File.WriteAllBytes(P(name), MetadataFixtures.Jpeg(fixture));
        return P(name);
    }

    private string Mp3(string name)
    {
        File.WriteAllBytes(P(name), MetadataFixtures.Mp3([("TIT2", "Song"), ("TPE1", "Artist")]));
        return P(name);
    }

    private static MetadataEdit Edit(params (MetadataField Field, string Value)[] changes) =>
        new(changes.ToDictionary(c => c.Field, c => c.Value));

    private static MetadataDocument Read(string path)
    {
        using var stream = ReadOnlyFile.TryOpen(path)!;
        return MetadataCodecs.For(path)!.Read(stream);
    }

    private string[] Leftovers() =>
        [.. Directory.EnumerateFiles(_root).Where(f => Path.GetFileName(f).Contains(".bertbrowser-")).Select(Path.GetFileName)!];

    // ---- planner --------------------------------------------------------------------------------

    private sealed class FakeProbe(Dictionary<string, FileAttributes> entries) : IMetadataProbe
    {
        public FileAttributes? AttributesOf(string path) =>
            entries.TryGetValue(path, out var found) ? found : null;

        public long LengthOf(string path) => 100;
    }

    [Fact]
    public void The_planner_takes_what_it_can_and_says_why_not_for_the_rest()
    {
        const FileAttributes recall = (FileAttributes)0x00400000;
        var planner = new MetadataEditPlanner(new FakeProbe(new()
        {
            [@"C:\p\a.jpg"] = FileAttributes.Archive,
            [@"C:\p\song.mp3"] = FileAttributes.Normal,
            [@"C:\p\folder.jpg"] = FileAttributes.Directory,
            [@"C:\p\link.jpg"] = FileAttributes.ReparsePoint,
            [@"C:\p\cloud.jpg"] = FileAttributes.Archive | recall,
            [@"C:\p\locked.jpg"] = FileAttributes.ReadOnly,
            [@"C:\p\notes.txt"] = FileAttributes.Normal,
        }));

        var plan = planner.Plan(
            [
                @"C:\p\a.jpg", @"C:\p\song.mp3", @"C:\p\folder.jpg", @"C:\p\link.jpg", @"C:\p\cloud.jpg",
                @"C:\p\locked.jpg", @"C:\p\notes.txt", @"C:\p\gone.jpg", @"C:\p\pack.zip\inner.jpg", @"C:\p\A.JPG",
            ],
            Edit((MetadataField.Title, "T"), (MetadataField.Album, "A")));

        Assert.Equal([@"C:\p\a.jpg", @"C:\p\song.mp3"], plan.Files.Select(f => f.Path));
        Assert.Equal(200, plan.RewriteBytes);

        // Each file gets only the part of the edit its kind can hold.
        Assert.Equal([MetadataField.Title], plan.Files[0].Edit.Changes.Keys);
        Assert.Equal([MetadataField.Title, MetadataField.Album], plan.Files[1].Edit.Changes.Keys.Order());

        var why = plan.Rejected.ToDictionary(r => Path.GetFileName(r.Path), r => r.Reason);
        Assert.Equal(MetadataRejection.IsFolder, why["folder.jpg"]);
        Assert.Equal(MetadataRejection.Link, why["link.jpg"]);
        Assert.Equal(MetadataRejection.CloudPlaceholder, why["cloud.jpg"]);
        Assert.Equal(MetadataRejection.ReadOnly, why["locked.jpg"]);
        Assert.Equal(MetadataRejection.Unsupported, why["notes.txt"]);
        Assert.Equal(MetadataRejection.Missing, why["gone.jpg"]);
        Assert.Equal(MetadataRejection.InsideArchive, why["inner.jpg"]);

        // Standing on an archive is not being inside one: a book is a zip, and is still a file.
        Assert.Null(MetadataEditPlanner.Refusal(@"C:\p\book.epub", FileAttributes.Normal));
        Assert.NotNull(MetadataEditPlanner.Refusal(@"C:\p\book.epub\cover.jpg", null));
        Assert.All(plan.Rejected, r => Assert.False(string.IsNullOrWhiteSpace(r.Message)));
    }

    [Fact]
    public void A_field_no_selected_file_can_hold_leaves_nothing_to_do()
    {
        var planner = new MetadataEditPlanner(new FakeProbe(new() { [@"C:\p\a.jpg"] = FileAttributes.Normal }));
        var plan = planner.Plan([@"C:\p\a.jpg"], Edit((MetadataField.Album, "A")));

        Assert.False(plan.HasWork);
        Assert.Equal(MetadataRejection.NothingApplies, Assert.Single(plan.Rejected).Reason);
    }

    // ---- executor -------------------------------------------------------------------------------

    [Fact]
    public void An_edit_lands_and_the_original_is_held_beside_it()
    {
        var photo = Jpeg("photo.jpg");
        var song = Mp3("song.mp3");
        var before = File.ReadAllBytes(photo);

        var outcome = _executor.Execute(_planner.Plan([photo, song], Edit((MetadataField.Title, "New"))));

        Assert.Empty(outcome.Failed);
        Assert.Equal(2, outcome.Edited.Count);
        Assert.Equal("New", Read(photo).Get(MetadataField.Title));
        Assert.Equal("New", Read(song).Get(MetadataField.Title));
        Assert.Equal("Artist", Read(song).Get(MetadataField.Artist));

        var held = outcome.Edited.Single(e => e.Path == photo);
        Assert.Contains(MetadataEditExecutor.ReplacedMarker, held.HeldPath);
        Assert.EndsWith(".jpg", held.HeldPath);
        Assert.Equal(before, File.ReadAllBytes(held.HeldPath));
        Assert.True(held.InPlace.Matches(photo));
    }

    [Fact]
    public void Undo_restores_the_exact_bytes_and_redo_brings_the_edit_back()
    {
        var photo = Jpeg("photo.jpg");
        var original = File.ReadAllBytes(photo);

        var outcome = _executor.Execute(_planner.Plan([photo], Edit((MetadataField.Title, "New"))));
        var edited = File.ReadAllBytes(photo);

        var undone = _executor.Undo(outcome.Edited);
        Assert.Empty(undone.Failed);
        Assert.Equal(original, File.ReadAllBytes(photo));
        Assert.Contains(MetadataEditExecutor.EditedMarker, undone.Swapped[0].HeldPath);

        var redone = _executor.Redo(undone.Swapped);
        Assert.Empty(redone.Failed);
        Assert.Equal(edited, File.ReadAllBytes(photo));

        MetadataEditExecutor.CommitStaging(redone.Swapped);
        Assert.Empty(Leftovers());
    }

    [Fact]
    public void The_swap_keeps_what_a_fresh_file_would_lose()
    {
        var photo = Jpeg("photo.jpg");
        var created = new DateTime(2019, 1, 2, 3, 4, 5, DateTimeKind.Utc);
        File.SetCreationTimeUtc(photo, created);
        File.WriteAllText(photo + ":Zone.Identifier", "[ZoneTransfer]\r\nZoneId=3\r\n");
        File.SetAttributes(photo, FileAttributes.Hidden | FileAttributes.Archive);

        var outcome = _executor.Execute(_planner.Plan([photo], Edit((MetadataField.Title, "New"))));

        Assert.Empty(outcome.Failed);
        Assert.Equal(created, File.GetCreationTimeUtc(photo));
        Assert.True(File.GetAttributes(photo).HasFlag(FileAttributes.Hidden));
        Assert.Equal("[ZoneTransfer]\r\nZoneId=3\r\n", File.ReadAllText(photo + ":Zone.Identifier"));

        // And the same again coming back.
        _executor.Undo(outcome.Edited);
        Assert.Equal(created, File.GetCreationTimeUtc(photo));
        Assert.True(File.GetAttributes(photo).HasFlag(FileAttributes.Hidden));
        Assert.Equal("[ZoneTransfer]\r\nZoneId=3\r\n", File.ReadAllText(photo + ":Zone.Identifier"));
    }

    [Fact]
    public void A_held_version_is_hidden_and_never_comes_back_hidden()
    {
        var photo = Jpeg("photo.jpg");

        var outcome = _executor.Execute(_planner.Plan([photo], Edit((MetadataField.Title, "New"))));
        Assert.True(File.GetAttributes(outcome.Edited[0].HeldPath).HasFlag(FileAttributes.Hidden));
        Assert.False(File.GetAttributes(photo).HasFlag(FileAttributes.Hidden));

        var undone = _executor.Undo(outcome.Edited);
        Assert.True(File.GetAttributes(undone.Swapped[0].HeldPath).HasFlag(FileAttributes.Hidden));
        Assert.False(File.GetAttributes(photo).HasFlag(FileAttributes.Hidden));

        var redone = _executor.Redo(undone.Swapped);
        Assert.False(File.GetAttributes(photo).HasFlag(FileAttributes.Hidden));

        MetadataEditExecutor.CommitStaging(redone.Swapped);
        Assert.Empty(Leftovers());
    }

    [Fact]
    public void One_bad_file_does_not_stop_the_others_and_is_left_exactly_as_it_was()
    {
        var good = Jpeg("good.jpg");
        var bad = P("bad.jpg");
        File.WriteAllText(bad, "this is not a picture at all");

        var outcome = _executor.Execute(_planner.Plan([bad, good], Edit((MetadataField.Title, "New"))));

        Assert.Equal(good, Assert.Single(outcome.Edited).Path);
        Assert.Equal(bad, Assert.Single(outcome.Failed).Path);
        Assert.Equal("this is not a picture at all", File.ReadAllText(bad));
        Assert.Equal("New", Read(good).Get(MetadataField.Title));

        // Only the good file's held original is beside it; the failed rewrite was cleaned up.
        Assert.Single(Leftovers());
    }

    [Fact]
    public void The_executor_asks_disk_again_rather_than_trusting_the_plan()
    {
        var photo = Jpeg("photo.jpg");
        var plan = _planner.Plan([photo], Edit((MetadataField.Title, "New")));

        File.SetAttributes(photo, FileAttributes.ReadOnly);
        var outcome = _executor.Execute(plan);

        Assert.Empty(outcome.Edited);
        Assert.Contains("read-only", Assert.Single(outcome.Failed).Message);
        Assert.Equal("Old title", Read(photo).Get(MetadataField.Title));
        Assert.Empty(Leftovers());
    }

    [Fact]
    public void A_cancel_before_the_start_changes_nothing()
    {
        var photo = Jpeg("photo.jpg");
        using var cancelled = new CancellationTokenSource();
        cancelled.Cancel();

        var outcome = _executor.Execute(_planner.Plan([photo], Edit((MetadataField.Title, "New"))), cancelled.Token);

        Assert.True(outcome.Cancelled);
        Assert.False(outcome.CanUndo);
        Assert.Equal("Old title", Read(photo).Get(MetadataField.Title));
        Assert.Empty(Leftovers());
    }

    [Fact]
    public void Undo_refuses_a_file_somebody_else_has_changed_since_and_keeps_the_held_copy()
    {
        var photo = Jpeg("photo.jpg");
        var outcome = _executor.Execute(_planner.Plan([photo], Edit((MetadataField.Title, "New"))));

        File.WriteAllBytes(photo, MetadataFixtures.Jpeg(MetadataFixtures.PlainJpeg));
        var undone = _executor.Undo(outcome.Edited);

        Assert.Empty(undone.Swapped);
        Assert.Contains("has changed since", Assert.Single(undone.Failed).Message);
        Assert.Equal(MetadataFixtures.Jpeg(MetadataFixtures.PlainJpeg), File.ReadAllBytes(photo));
        Assert.True(File.Exists(outcome.Edited[0].HeldPath));
    }

    [Fact]
    public void A_date_shift_moves_each_picture_from_its_own_date_and_skips_one_with_none()
    {
        var camera = Jpeg("camera.jpg");
        var plain = Jpeg("plain.jpg", MetadataFixtures.PlainJpeg);
        var song = Mp3("song.mp3");

        var plan = _planner.Plan(
            [camera, plain, song], MetadataEdit.None with { ShiftDateTaken = TimeSpan.FromHours(-5) });

        // A song has no date taken; the planner drops it rather than the executor failing it.
        Assert.Equal([camera, plain], plan.Files.Select(f => f.Path));
        Assert.Equal(MetadataRejection.NothingApplies, Assert.Single(plan.Rejected).Reason);

        var outcome = _executor.Execute(plan);

        Assert.Equal(camera, Assert.Single(outcome.Edited).Path);
        Assert.Equal("2021-06-04 23:03:02", Read(camera).Get(MetadataField.DateTaken));
        Assert.Contains("no date taken", Assert.Single(outcome.Failed).Message);
    }

    [Fact]
    public void Removing_a_location_is_undoable_like_any_other_edit()
    {
        var photo = Jpeg("photo.jpg");
        var outcome = _executor.Execute(_planner.Plan([photo], MetadataEdit.None with { RemoveLocation = true }));

        Assert.False(Read(photo).HasLocation);
        _executor.Undo(outcome.Edited);
        Assert.True(Read(photo).HasLocation);
    }

    [Fact]
    public void An_edit_that_changes_nothing_is_neither_a_change_nor_a_failure()
    {
        var plain = Jpeg("plain.jpg", MetadataFixtures.PlainJpeg);
        var camera = Jpeg("camera.jpg");
        var before = EntryStamp.Of(plain)!.Value;

        var outcome = _executor.Execute(_planner.Plan(
            [plain, camera], MetadataEdit.None with { RemoveLocation = true }));

        Assert.Equal(camera, Assert.Single(outcome.Edited).Path);
        Assert.Equal([plain], outcome.Unchanged);
        Assert.Empty(outcome.Failed);
        Assert.True(before.Matches(plain));

        // Writing a value a file already holds is the same non-event — the second time. The first
        // is a real change: the camera wrote the title in one of its two tags and this fills in
        // the other.
        var first = _executor.Execute(_planner.Plan([camera], Edit((MetadataField.Title, "Old title"))));
        Assert.True(first.CanUndo);

        var again = _executor.Execute(_planner.Plan([camera], Edit((MetadataField.Title, "Old title"))));
        Assert.False(again.CanUndo);
        Assert.Equal([camera], again.Unchanged);
        Assert.Equal(2, Leftovers().Length);
    }

    [Fact]
    public void Committing_erases_only_files_named_the_way_this_names_them()
    {
        var photo = Jpeg("photo.jpg");
        MetadataEditExecutor.CommitStaging([new HeldVersion(photo, photo, EntryStamp.Of(photo)!.Value)]);

        Assert.True(File.Exists(photo));
    }

    // ---- undo record ----------------------------------------------------------------------------

    [Fact]
    public async Task The_record_steps_back_and_forth_and_retiring_it_erases_what_it_held()
    {
        var host = new PlainUndoHost(
            new TransferExecutor(), new DeleteExecutor(new FileSystemDeleteProbe(), [], stagingRoot: _root),
            new RenameExecutor(), new ArchiveEditExecutor(new SharpCompressArchiveReader()));

        var photo = Jpeg("photo.jpg");
        var song = Mp3("song.mp3");
        var record = MetadataEditRecord.For(
            _executor.Execute(_planner.Plan([photo, song], Edit((MetadataField.Title, "New")))))!;

        Assert.Equal(UndoKind.MetadataEdit, record.Kind);
        Assert.True(record.IsApplied);
        Assert.Equal(2, record.ItemCount);
        Assert.Equal(2, record.Held.Count);
        Assert.Equal("Edit metadata of 2 files", record.Description);

        var undone = await record.StepAsync(host);
        Assert.True(undone.Report.Clean);
        Assert.False(undone.Next.IsApplied);
        Assert.Equal("Old title", Read(photo).Get(MetadataField.Title));
        Assert.Equal("Song", Read(song).Get(MetadataField.Title));
        Assert.Equal([_root], undone.Refresh.Directories);

        var redone = await undone.Next.StepAsync(host);
        Assert.True(redone.Next.IsApplied);
        Assert.Equal("New", Read(photo).Get(MetadataField.Title));

        redone.Next.Retire();
        Assert.Empty(Leftovers());
    }

    [Fact]
    public void An_edit_that_changed_nothing_is_not_a_history_entry()
    {
        Assert.Null(MetadataEditRecord.For(MetadataEditOutcome.Empty));
    }
}
