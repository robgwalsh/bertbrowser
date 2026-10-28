//! What the preview pane plays after the current file, whether by its skip button or because the
//! current one ended with auto-advance on.
//!
//! The list is the file list's rows in the order the user sees them, so "next" follows whatever
//! sort is showing rather than the directory's own order. A video continues to the next video and
//! a song to the next song: a folder of clips with a soundtrack beside them should not break into
//! the MP3 halfway through. There is no wrap-around: reaching the end stops, the way a playlist
//! does, rather than starting the folder over while nobody is watching.
//! Spec: Core/Services/Preview/MediaPlaylist.cs.

use super::{PreviewClassifier, PreviewKind};

pub struct MediaPlaylist;

impl MediaPlaylist {
    /// `names` are the rows' names in display order; folders are passed as None, since a folder
    /// named `clip.mp4` is not a video. `current` is the index of the file playing now. Returns
    /// the index to play next, or None when nothing after `current` fits.
    pub fn next(names: &[Option<&str>], current: usize) -> Option<usize> {
        let playing = (*names.get(current)?)?;
        if PreviewClassifier::kind_for(playing) != PreviewKind::Media {
            return None;
        }

        let video = PreviewClassifier::is_video(playing);
        names
            .iter()
            .enumerate()
            .skip(current + 1)
            .find(|(_, name)| {
                name.is_some_and(|name| {
                    PreviewClassifier::kind_for(name) == PreviewKind::Media
                        && PreviewClassifier::is_video(name) == video
                })
            })
            .map(|(i, _)| i)
    }
}
