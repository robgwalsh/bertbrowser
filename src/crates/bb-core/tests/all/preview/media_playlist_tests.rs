// Port of tests/BertBrowser.Core.Tests/MediaPlaylistTests.cs
use bb_core::preview::MediaPlaylist;

#[test]
fn skips_what_is_not_a_video_to_reach_the_next_one() {
    let rows = [
        Some("a.mp4"),
        Some("notes.txt"),
        Some("cover.jpg"),
        Some("b.MKV"),
    ];
    assert_eq!(Some(3), MediaPlaylist::next(&rows, 0));
}

#[test]
fn a_video_does_not_continue_into_audio() {
    let rows = [Some("a.mp4"), Some("theme.mp3"), Some("b.mov")];
    assert_eq!(Some(2), MediaPlaylist::next(&rows, 0));
}

#[test]
fn a_song_continues_to_the_next_song() {
    let rows = [Some("01.flac"), Some("clip.mp4"), Some("02.flac")];
    assert_eq!(Some(2), MediaPlaylist::next(&rows, 0));
}

#[test]
fn the_end_of_the_list_stops_rather_than_wrapping() {
    let rows = [Some("a.mp4"), Some("b.mp4")];
    assert_eq!(None, MediaPlaylist::next(&rows, 1));
}

#[test]
fn a_folder_is_never_next_whatever_it_is_called() {
    // Folders arrive as None, so a directory named like a video is passed over.
    let rows = [Some("a.mp4"), None, Some("b.mp4")];
    assert_eq!(Some(2), MediaPlaylist::next(&rows, 0));
}

#[test]
fn an_index_outside_the_list_has_no_next() {
    // The C# also asks about -1; an unsigned index makes that unrepresentable, so the row past
    // the end stands for both.
    let rows = [Some("a.mp4"), Some("b.mp4")];
    for current in [5, usize::MAX] {
        assert_eq!(
            None,
            MediaPlaylist::next(&rows, current),
            "current {current}"
        );
    }
}

#[test]
fn something_that_is_not_media_has_no_next() {
    let rows = [Some("readme.txt"), Some("a.mp4")];
    assert_eq!(None, MediaPlaylist::next(&rows, 0));
}
