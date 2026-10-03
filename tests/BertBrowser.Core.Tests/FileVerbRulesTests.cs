using BertBrowser.Core.Services.Commands;
using Xunit;

namespace BertBrowser.Core.Tests;

public class FileVerbRulesTests
{
    /// <summary>A selection of <paramref name="files"/> files and <paramref name="folders"/>
    /// folders, in an ordinary folder unless said otherwise.</summary>
    private static FileVerbSnapshot Tab(
        int files = 0, int folders = 0, bool inArchive = false, bool search = false,
        bool hasFolder = true, bool comparing = false, bool singleArchive = false,
        bool allBookmarked = false, bool locked = false, bool clipboard = false, bool elevatable = false) =>
        new(files + folders, files, inArchive, search, hasFolder, comparing, singleArchive,
            allBookmarked, locked, () => clipboard, () => elevatable);

    private static bool On(FileVerb verb, FileVerbSnapshot s) => FileVerbRules.For(verb, s).Enabled;

    private static string Header(FileVerb verb, FileVerbSnapshot s) => FileVerbRules.For(verb, s).Header;

    [Fact]
    public void Every_verb_has_a_rule_and_an_unavailable_one_says_why()
    {
        var nothing = Tab(hasFolder: false);

        Assert.All(Enum.GetValues<FileVerb>(), verb =>
        {
            var state = FileVerbRules.For(verb, nothing);
            Assert.False(string.IsNullOrWhiteSpace(state.Header));
            Assert.Equal(state.Enabled, state.Reason is null);
        });
    }

    // --- Inside an archive ---

    [Theory]
    [InlineData(FileVerb.Cut)]
    [InlineData(FileVerb.Copy)]
    [InlineData(FileVerb.Paste)]
    [InlineData(FileVerb.New)]
    [InlineData(FileVerb.Compress)]
    [InlineData(FileVerb.Checksum)]
    [InlineData(FileVerb.VerifyChecksums)]
    [InlineData(FileVerb.Duplicates)]
    [InlineData(FileVerb.Changes)]
    [InlineData(FileVerb.Bookmark)]
    [InlineData(FileVerb.DeletePermanently)]
    [InlineData(FileVerb.OpenInTerminal)]
    [InlineData(FileVerb.OpenInVSCode)]
    public void Nothing_that_needs_a_real_path_runs_inside_an_archive(FileVerb verb)
    {
        Assert.False(On(verb, Tab(files: 1, inArchive: true, clipboard: true)));
        Assert.False(On(verb, Tab(inArchive: true, clipboard: true)));
    }

    [Fact]
    public void Two_files_are_not_comparable_inside_an_archive()
    {
        Assert.True(On(FileVerb.CompareFiles, Tab(files: 2)));
        Assert.False(On(FileVerb.CompareFiles, Tab(files: 2, inArchive: true)));
    }

    [Fact]
    public void Disk_usage_stays_on_inside_an_archive_because_every_size_there_is_exact()
    {
        Assert.True(On(FileVerb.DiskUsage, Tab(inArchive: true)));
    }

    [Fact]
    public void Rename_and_delete_stay_on_inside_an_archive_but_rename_takes_one_at_a_time()
    {
        Assert.True(On(FileVerb.Rename, Tab(files: 1, inArchive: true)));
        Assert.True(On(FileVerb.Delete, Tab(files: 3, inArchive: true)));
        Assert.False(On(FileVerb.Rename, Tab(files: 2, inArchive: true)));
        Assert.True(On(FileVerb.Rename, Tab(files: 2)));
    }

    [Fact]
    public void Extract_is_the_one_write_verb_more_available_inside_than_outside()
    {
        var inside = FileVerbRules.For(FileVerb.ExtractHere, Tab(inArchive: true));
        var onArchive = FileVerbRules.For(FileVerb.ExtractTo, Tab(files: 1, singleArchive: true));
        var onText = FileVerbRules.For(FileVerb.ExtractHere, Tab(files: 1));

        Assert.True(inside is { Visible: true, Enabled: true });
        Assert.True(onArchive is { Visible: true, Enabled: true });
        Assert.True(onText is { Visible: false, Enabled: false });
        Assert.Equal("Extract 3 item(s) here", Header(FileVerb.ExtractHere, Tab(files: 3, inArchive: true)));
        Assert.Equal("Extract here", Header(FileVerb.ExtractHere, Tab(inArchive: true)));
    }

    // --- Search results and flat views ---

    [Fact]
    public void A_search_result_has_no_folder_to_create_in_or_compress()
    {
        Assert.False(On(FileVerb.New, Tab(search: true)));
        Assert.False(On(FileVerb.Compress, Tab(files: 1, search: true)));
        Assert.False(On(FileVerb.New, Tab(hasFolder: false)));
        Assert.True(On(FileVerb.New, Tab()));
        Assert.True(On(FileVerb.Compress, Tab()));
    }

    // --- Selection shape ---

    [Theory]
    [InlineData(FileVerb.Open)]
    [InlineData(FileVerb.Cut)]
    [InlineData(FileVerb.Copy)]
    [InlineData(FileVerb.CopyPath)]
    [InlineData(FileVerb.CopyName)]
    [InlineData(FileVerb.Rename)]
    [InlineData(FileVerb.Delete)]
    [InlineData(FileVerb.DeletePermanently)]
    [InlineData(FileVerb.Bookmark)]
    [InlineData(FileVerb.Properties)]
    [InlineData(FileVerb.Checksum)]
    public void Verbs_about_the_selection_need_one(FileVerb verb)
    {
        Assert.False(On(verb, Tab()));
        Assert.True(On(verb, Tab(files: 1)));
    }

    [Fact]
    public void A_checksum_is_of_files_only()
    {
        Assert.True(On(FileVerb.Checksum, Tab(files: 3)));
        Assert.False(On(FileVerb.Checksum, Tab(files: 3, folders: 1)));
        Assert.False(On(FileVerb.Checksum, Tab(folders: 1)));
        Assert.Equal("Checksums of 3 files…", Header(FileVerb.Checksum, Tab(files: 3)));
        Assert.Equal("Checksum…", Header(FileVerb.Checksum, Tab(files: 1)));
    }

    [Theory]
    [InlineData(2, 0, true)]
    [InlineData(1, 0, false)]
    [InlineData(3, 0, false)]
    [InlineData(1, 1, false)]
    [InlineData(0, 2, false)]
    public void Comparing_files_takes_exactly_two_files(int files, int folders, bool expected)
    {
        Assert.Equal(expected, On(FileVerb.CompareFiles, Tab(files, folders)));
    }

    [Fact]
    public void Opening_in_a_tab_or_pane_needs_a_folder_in_the_selection()
    {
        Assert.False(On(FileVerb.OpenInNewTab, Tab(files: 2)));
        Assert.True(On(FileVerb.OpenInNewTab, Tab(files: 2, folders: 1)));
        Assert.True(On(FileVerb.OpenInNewPane, Tab(folders: 1)));
        Assert.Equal("Open 3 folders in new tabs", Header(FileVerb.OpenInNewTab, Tab(folders: 3)));
        Assert.Equal("Open in new tab", Header(FileVerb.OpenInNewTab, Tab(folders: 1)));
    }

    [Theory]
    [InlineData(FileVerb.DiskUsage)]
    [InlineData(FileVerb.Duplicates)]
    [InlineData(FileVerb.Changes)]
    public void Folder_tools_take_one_folder_or_the_folder_being_shown(FileVerb verb)
    {
        Assert.True(On(verb, Tab()));
        Assert.True(On(verb, Tab(folders: 1)));
        Assert.False(On(verb, Tab(files: 1)));
        Assert.False(On(verb, Tab(folders: 2)));
    }

    [Fact]
    public void Terminal_and_editor_fall_back_to_the_folder_being_shown()
    {
        Assert.True(On(FileVerb.OpenInTerminal, Tab()));
        Assert.True(On(FileVerb.OpenInVSCode, Tab(files: 1, hasFolder: false)));
        Assert.False(On(FileVerb.OpenInTerminal, Tab(hasFolder: false)));
    }

    [Fact]
    public void Run_as_administrator_is_for_one_item_with_something_to_elevate()
    {
        Assert.True(On(FileVerb.RunAsAdmin, Tab(files: 1, elevatable: true)));
        Assert.False(On(FileVerb.RunAsAdmin, Tab(files: 1, elevatable: false)));
        Assert.False(On(FileVerb.RunAsAdmin, Tab(files: 2, elevatable: true)));
        Assert.False(On(FileVerb.RunAsAdmin, Tab(elevatable: true)));
    }

    [Fact]
    public void The_registry_is_only_asked_about_a_single_item()
    {
        var asked = 0;
        var two = new FileVerbSnapshot(2, 2, false, false, true, CanRunElevated: () => { asked++; return true; });

        FileVerbRules.For(FileVerb.RunAsAdmin, two);
        FileVerbRules.For(FileVerb.Rename, two);

        Assert.Equal(0, asked);
    }

    [Fact]
    public void The_clipboard_is_only_asked_by_paste()
    {
        var asked = 0;
        var tab = new FileVerbSnapshot(1, 1, false, false, true, ClipboardHasFiles: () => { asked++; return true; });

        foreach (var verb in Enum.GetValues<FileVerb>().Where(v => v != FileVerb.Paste))
            FileVerbRules.For(verb, tab);
        Assert.Equal(0, asked);

        Assert.True(On(FileVerb.Paste, tab));
        Assert.Equal(1, asked);
    }

    // --- Comparison ---

    [Fact]
    public void Settling_by_content_exists_only_while_comparing()
    {
        var idle = FileVerbRules.For(FileVerb.SettleByContent, Tab(files: 1));
        var comparing = FileVerbRules.For(FileVerb.SettleByContent, Tab(files: 2, comparing: true));
        var foldersOnly = FileVerbRules.For(FileVerb.SettleByContent, Tab(folders: 1, comparing: true));

        Assert.True(idle is { Visible: false, Enabled: false });
        Assert.True(comparing is { Visible: true, Enabled: true, Header: "Settle these by content" });
        Assert.True(foldersOnly is { Visible: true, Enabled: false });
    }

    [Fact]
    public void Compare_is_never_off_and_says_what_pressing_it_will_do()
    {
        Assert.Equal("Compare with other pane", Header(FileVerb.ComparePanes, Tab()));
        Assert.Equal("Stop comparing", Header(FileVerb.ComparePanes, Tab(comparing: true)));
        Assert.True(On(FileVerb.ComparePanes, Tab(inArchive: true, hasFolder: false)));
    }

    // --- Wording ---

    [Fact]
    public void Headers_count_what_they_will_act_on()
    {
        Assert.Equal("Rename 4 items…", Header(FileVerb.Rename, Tab(files: 4)));
        Assert.Equal("Rename…", Header(FileVerb.Rename, Tab(files: 1)));
        Assert.Equal("Delete 2 items…", Header(FileVerb.Delete, Tab(files: 2)));
        Assert.Equal("Copy as paths", Header(FileVerb.CopyPath, Tab(files: 2)));
        Assert.Equal("Copy names", Header(FileVerb.CopyName, Tab(files: 2)));
        Assert.Equal("Compress 1,200 items…", Header(FileVerb.Compress, Tab(files: 1200)));
    }

    [Fact]
    public void Bookmark_becomes_remove_only_when_everything_selected_is_bookmarked()
    {
        Assert.Equal("Bookmark", Header(FileVerb.Bookmark, Tab(files: 2)));
        Assert.Equal("Remove bookmark", Header(FileVerb.Bookmark, Tab(files: 2, allBookmarked: true)));
    }

    [Fact]
    public void Unlock_is_for_a_locked_archive()
    {
        Assert.True(On(FileVerb.UnlockArchive, Tab(inArchive: true, locked: true)));
        Assert.False(On(FileVerb.UnlockArchive, Tab(inArchive: true)));
    }
}
