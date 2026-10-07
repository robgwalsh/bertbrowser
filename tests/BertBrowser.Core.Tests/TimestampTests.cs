using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Metadata;
using BertBrowser.Core.Services.Rename;
using BertBrowser.Core.Services.Timestamps;
using BertBrowser.Core.Services.Transfer;
using BertBrowser.Core.Services.UndoHistory;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>Setting a file's own dates: the planner against a fake disk, the rest against a real one.</summary>
public sealed class TimestampTests : IDisposable
{
    private static readonly DateTime Noon = new(2020, 5, 6, 12, 0, 0, DateTimeKind.Utc);

    private readonly string _root;
    private readonly TimestampPlanner _planner = new();
    private readonly TimestampExecutor _executor = new();

    public TimestampTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-dates-{Guid.NewGuid():N}");
        Directory.CreateDirectory(_root);
    }

    public void Dispose()
    {
        try
        {
            Directory.Delete(_root, recursive: true);
        }
        catch (IOException)
        {
        }
    }

    private string P(params string[] parts) => Path.Combine([_root, .. parts]);

    private string File_(string name)
    {
        File.WriteAllText(P(name), name);
        File.SetCreationTimeUtc(P(name), Noon);
        File.SetLastWriteTimeUtc(P(name), Noon);
        return P(name);
    }

    private sealed class FakeProbe : ITimestampProbe
    {
        public Dictionary<string, TimestampEntry> Entries { get; } = new(StringComparer.OrdinalIgnoreCase);

        public Dictionary<string, DateTime> Taken { get; } = new(StringComparer.OrdinalIgnoreCase);

        public TimestampEntry? Find(string path) => Entries.GetValueOrDefault(path);

        public DateTime? DateTakenOf(string path) => Taken.TryGetValue(path, out var taken) ? taken : null;
    }

    private static FakeProbe Disk()
    {
        var probe = new FakeProbe();
        probe.Entries[@"C:\p\a.jpg"] = new(FileAttributes.Archive, Noon, Noon.AddDays(-1));
        probe.Entries[@"C:\p\folder"] = new(FileAttributes.Directory, Noon, Noon);
        probe.Entries[@"C:\p\link"] = new(FileAttributes.ReparsePoint, Noon, Noon);
        probe.Taken[@"C:\p\a.jpg"] = new DateTime(2021, 6, 5, 4, 3, 2);
        return probe;
    }

    // ---- planner --------------------------------------------------------------------------------

    [Fact]
    public void A_value_is_local_time_and_only_the_ticked_dates_are_planned()
    {
        var local = new DateTime(2022, 2, 3, 4, 5, 6);
        var plan = new TimestampPlanner(Disk()).Plan(
            [@"C:\p\a.jpg", @"C:\p\folder"], new TimestampChange(true, false, TimestampSource.Value, Value: local));

        Assert.Equal(2, plan.Items.Count);
        Assert.All(plan.Items, i => Assert.Equal(DateTime.SpecifyKind(local, DateTimeKind.Local).ToUniversalTime(), i.ModifiedUtc));
        Assert.All(plan.Items, i => Assert.Null(i.CreatedUtc));
        Assert.True(plan.Items[1].IsDirectory);
    }

    [Fact]
    public void A_shift_moves_each_item_from_its_own_dates()
    {
        var plan = new TimestampPlanner(Disk()).Plan(
            [@"C:\p\a.jpg"], new TimestampChange(true, true, TimestampSource.Shift, Offset: TimeSpan.FromHours(-2)));

        var item = Assert.Single(plan.Items);
        Assert.Equal(Noon.AddHours(-2), item.ModifiedUtc);
        Assert.Equal(Noon.AddDays(-1).AddHours(-2), item.CreatedUtc);
    }

    [Fact]
    public void Date_taken_is_per_picture_and_anything_without_one_is_left_out_by_name()
    {
        var plan = new TimestampPlanner(Disk()).Plan(
            [@"C:\p\a.jpg", @"C:\p\folder"], new TimestampChange(true, false, TimestampSource.DateTaken));

        var taken = DateTime.SpecifyKind(new DateTime(2021, 6, 5, 4, 3, 2), DateTimeKind.Local).ToUniversalTime();
        Assert.Equal(taken, Assert.Single(plan.Items).ModifiedUtc);
        Assert.Equal(TimestampRejection.NoDateTaken, Assert.Single(plan.Rejected).Reason);
    }

    [Fact]
    public void What_cannot_be_dated_is_refused_with_a_reason()
    {
        var change = new TimestampChange(true, false, TimestampSource.Value, Value: new DateTime(2022, 1, 1));
        var plan = new TimestampPlanner(Disk()).Plan(
            [@"C:\p\link", @"C:\p\gone.txt", @"C:\p\pack.zip\inner.txt"], change);

        Assert.False(plan.HasWork);
        Assert.Equal(
            [TimestampRejection.Link, TimestampRejection.Missing, TimestampRejection.InsideArchive],
            plan.Rejected.Select(r => r.Reason));
    }

    [Fact]
    public void A_date_windows_cannot_store_and_a_date_already_there_are_both_left_out()
    {
        var planner = new TimestampPlanner(Disk());

        var ancient = planner.Plan([@"C:\p\a.jpg"],
            new TimestampChange(true, false, TimestampSource.Value, Value: new DateTime(1500, 1, 1)));
        Assert.Equal(TimestampRejection.OutOfRange, Assert.Single(ancient.Rejected).Reason);

        var same = planner.Plan([@"C:\p\a.jpg"],
            new TimestampChange(true, false, TimestampSource.Value, Value: Noon.ToLocalTime()));
        Assert.Equal(TimestampRejection.Unchanged, Assert.Single(same.Rejected).Reason);

        Assert.False(planner.Plan([@"C:\p\a.jpg"], new TimestampChange(false, false, TimestampSource.DateTaken)).HasWork);
    }

    // ---- executor -------------------------------------------------------------------------------

    [Fact]
    public void Dates_are_set_on_files_and_folders_and_reverting_is_its_own_redo()
    {
        var file = File_("a.txt");
        var folder = P("sub");
        Directory.CreateDirectory(folder);
        var folderCreated = Directory.GetCreationTimeUtc(folder);

        var when = new DateTime(2019, 3, 4, 5, 6, 7);
        var whenUtc = DateTime.SpecifyKind(when, DateTimeKind.Local).ToUniversalTime();

        var outcome = _executor.Execute(_planner.Plan(
            [file, folder], new TimestampChange(true, false, TimestampSource.Value, Value: when)));

        Assert.Empty(outcome.Failed);
        Assert.Equal(whenUtc, File.GetLastWriteTimeUtc(file));
        Assert.Equal(whenUtc, Directory.GetLastWriteTimeUtc(folder));
        Assert.Equal(Noon, File.GetCreationTimeUtc(file));
        Assert.Equal(folderCreated, Directory.GetCreationTimeUtc(folder));
        Assert.Equal("a.txt", File.ReadAllText(file));

        var undone = _executor.Revert(outcome.Stamped);
        Assert.Empty(undone.Failed);
        Assert.Equal(Noon, File.GetLastWriteTimeUtc(file));

        var redone = _executor.Revert(undone.Stamped);
        Assert.Empty(redone.Failed);
        Assert.Equal(whenUtc, File.GetLastWriteTimeUtc(file));
    }

    [Fact]
    public void A_picture_can_be_dated_from_what_its_camera_wrote()
    {
        var photo = P("camera.jpg");
        File.WriteAllBytes(photo, MetadataFixtures.Jpeg(MetadataFixtures.CameraJpeg));
        var plain = File_("notes.txt");

        var outcome = _executor.Execute(_planner.Plan(
            [photo, plain], new TimestampChange(true, true, TimestampSource.DateTaken)));

        var taken = new DateTime(2021, 6, 5, 4, 3, 2);
        Assert.Equal(photo, Assert.Single(outcome.Stamped).Path);
        Assert.Equal(taken, File.GetLastWriteTime(photo));
        Assert.Equal(taken, File.GetCreationTime(photo));
        Assert.Equal(Noon, File.GetLastWriteTimeUtc(plain));
    }

    [Fact]
    public void A_revert_leaves_alone_a_file_something_else_has_written_since()
    {
        var file = File_("a.txt");
        var outcome = _executor.Execute(_planner.Plan(
            [file], new TimestampChange(true, false, TimestampSource.Shift, Offset: TimeSpan.FromDays(1))));

        var later = Noon.AddDays(40);
        File.SetLastWriteTimeUtc(file, later);

        var undone = _executor.Revert(outcome.Stamped);
        Assert.Empty(undone.Stamped);
        Assert.Contains("has changed since", Assert.Single(undone.Failed).Message);
        Assert.Equal(later, File.GetLastWriteTimeUtc(file));
    }

    [Fact]
    public void One_missing_file_does_not_stop_the_others()
    {
        var kept = File_("kept.txt");
        var gone = File_("gone.txt");
        var plan = _planner.Plan(
            [gone, kept], new TimestampChange(true, false, TimestampSource.Shift, Offset: TimeSpan.FromHours(1)));

        File.Delete(gone);
        var outcome = _executor.Execute(plan);

        Assert.Equal(kept, Assert.Single(outcome.Stamped).Path);
        Assert.Equal(gone, Assert.Single(outcome.Failed).Path);
        Assert.Equal(Noon.AddHours(1), File.GetLastWriteTimeUtc(kept));
    }

    // ---- undo record ----------------------------------------------------------------------------

    [Fact]
    public async Task The_record_holds_nothing_and_steps_both_ways()
    {
        var host = new PlainUndoHost(
            new TransferExecutor(), new DeleteExecutor(new FileSystemDeleteProbe(), [], stagingRoot: _root),
            new RenameExecutor(), new ArchiveEditExecutor(new SharpCompressArchiveReader()));

        var file = File_("a.txt");
        var record = TimestampRecord.For(_executor.Execute(_planner.Plan(
            [file], new TimestampChange(true, false, TimestampSource.Shift, Offset: TimeSpan.FromHours(3)))))!;

        Assert.Equal(UndoKind.Timestamps, record.Kind);
        Assert.Empty(record.Held);
        Assert.Equal("Change the date of a.txt", record.Description);

        var undone = await record.StepAsync(host);
        Assert.True(undone.Report.Clean);
        Assert.False(undone.Next.IsApplied);
        Assert.Equal(Noon, File.GetLastWriteTimeUtc(file));

        var redone = await undone.Next.StepAsync(host);
        Assert.True(redone.Next.IsApplied);
        Assert.Equal(Noon.AddHours(3), File.GetLastWriteTimeUtc(file));

        Assert.Null(TimestampRecord.For(TimestampOutcome.Empty));
    }
}
