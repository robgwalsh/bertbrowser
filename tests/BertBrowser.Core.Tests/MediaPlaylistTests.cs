using BertBrowser.Core.Services.Preview;
using Xunit;

namespace BertBrowser.Core.Tests;

public class MediaPlaylistTests
{
    [Fact]
    public void SkipsWhatIsNotAVideoToReachTheNextOne()
    {
        string?[] rows = ["a.mp4", "notes.txt", "cover.jpg", "b.MKV"];
        Assert.Equal(3, MediaPlaylist.Next(rows, 0));
    }

    [Fact]
    public void AVideoDoesNotContinueIntoAudio()
    {
        string?[] rows = ["a.mp4", "theme.mp3", "b.mov"];
        Assert.Equal(2, MediaPlaylist.Next(rows, 0));
    }

    [Fact]
    public void ASongContinuesToTheNextSong()
    {
        string?[] rows = ["01.flac", "clip.mp4", "02.flac"];
        Assert.Equal(2, MediaPlaylist.Next(rows, 0));
    }

    [Fact]
    public void TheEndOfTheListStopsRatherThanWrapping()
    {
        string?[] rows = ["a.mp4", "b.mp4"];
        Assert.Equal(-1, MediaPlaylist.Next(rows, 1));
    }

    [Fact]
    public void AFolderIsNeverNextWhateverItIsCalled()
    {
        // Folders arrive as null, so a directory named like a video is passed over.
        string?[] rows = ["a.mp4", null, "b.mp4"];
        Assert.Equal(2, MediaPlaylist.Next(rows, 0));
    }

    [Theory]
    [InlineData(-1)]
    [InlineData(5)]
    public void AnIndexOutsideTheListHasNoNext(int current)
    {
        string?[] rows = ["a.mp4", "b.mp4"];
        Assert.Equal(-1, MediaPlaylist.Next(rows, current));
    }

    [Fact]
    public void SomethingThatIsNotMediaHasNoNext()
    {
        string?[] rows = ["readme.txt", "a.mp4"];
        Assert.Equal(-1, MediaPlaylist.Next(rows, 0));
    }
}
