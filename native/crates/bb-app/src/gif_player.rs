//! An animated GIF whose frames arrive while it plays. Spec: PreviewAnimation and
//! PreviewPaneViewModel.StartAnimation in the C# app.
//!
//! **One thread decodes, and the caller gets the first frame before the rest exist.** Only the
//! delays are read up front (a walk over the block structure, no pixels), because the seek bar
//! needs the whole length from the start. Each frame is a whole composed picture (BGRA, the
//! canvas's size), stored before the ready count moves past it, so any frame below the count is
//! safe to read. A seek to a frame that has not arrived yet shows the last one that has, and the
//! frame asked for is filled in once it lands. Decoding ends early on a frame that will not decode
//! or when the player is closed; playback then loops over what there is.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use bb_core::cancel::CancelToken;
use bb_core::fs::ReadOnlyFile;
use bb_core::preview::animation::{
    AnimationLimits, AnimationTimeline, GifDecoder, GifFrameCompositor,
};
use std::io::Read;

#[derive(Debug, PartialEq, Eq)]
pub enum GifOpenError {
    /// Not a GIF this decoder can read, or the file could not be opened.
    Unreadable,
    /// One frame: just a picture.
    Still,
    /// More frames than are worth holding decoded; the host shows the still frame.
    TooLarge { frames: usize },
}

struct Shared {
    frames: Mutex<Vec<Arc<Vec<u8>>>>,
    arrived: Condvar,
    stopped: AtomicBool,
    finished: AtomicBool,
    cancel: CancelToken,
}

pub struct GifPlayer {
    pub width: u32,
    pub height: u32,
    timeline: AnimationTimeline,
    shared: Arc<Shared>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl GifPlayer {
    /// Reads the file, checks the size gate and starts decoding. Returns once frame 0 is ready.
    pub fn open(path: &str) -> Result<Arc<GifPlayer>, GifOpenError> {
        let mut file = ReadOnlyFile::try_open(path, false).ok_or(GifOpenError::Unreadable)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|_| GifOpenError::Unreadable)?;
        drop(file); // nothing holds the file open while it plays
        Self::from_bytes(bytes)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Arc<GifPlayer>, GifOpenError> {
        let (header, delays) = GifDecoder::scan(&bytes).map_err(|_| GifOpenError::Unreadable)?;
        if header.frame_count < 2 {
            return Err(GifOpenError::Still);
        }
        if !AnimationLimits::allows(header.frame_count, header.width, header.height) {
            return Err(GifOpenError::TooLarge {
                frames: header.frame_count,
            });
        }
        let timeline = AnimationTimeline::new(&delays).ok_or(GifOpenError::Unreadable)?;
        let shared = Arc::new(Shared {
            frames: Mutex::new(Vec::with_capacity(header.frame_count)),
            arrived: Condvar::new(),
            stopped: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            cancel: CancelToken::new(),
        });

        let worker = shared.clone();
        let (width, height) = (header.width, header.height);
        std::thread::Builder::new()
            .name("bb-gif".into())
            .spawn(move || {
                let mut compositor = GifFrameCompositor::new(width, height);
                let result = GifDecoder::decode_each(&bytes, &worker.cancel, &mut |frame, _| {
                    let picture = Arc::new(compositor.next(&frame));
                    lock(&worker.frames).push(picture);
                    worker.arrived.notify_all();
                    !worker.stopped.load(Ordering::Relaxed)
                });
                if result.is_err() {
                    worker.stopped.store(true, Ordering::Relaxed);
                }
                worker.finished.store(true, Ordering::Relaxed);
                worker.arrived.notify_all();
            })
            .map_err(|_| GifOpenError::Unreadable)?;

        // The caller's frame 0: wait for the first picture, and for nothing else.
        {
            let mut frames = lock(&shared.frames);
            while frames.is_empty() && !shared.finished.load(Ordering::Relaxed) {
                frames = shared
                    .arrived
                    .wait_timeout(frames, std::time::Duration::from_millis(50))
                    .unwrap_or_else(|p| p.into_inner())
                    .0;
            }
            if frames.is_empty() {
                return Err(GifOpenError::Unreadable);
            }
        }
        Ok(Arc::new(GifPlayer {
            width,
            height,
            timeline,
            shared,
        }))
    }

    pub fn frame_count(&self) -> usize {
        self.timeline.frame_count()
    }

    /// One pass, in milliseconds.
    pub fn duration_ms(&self) -> i64 {
        self.timeline.duration()
    }

    /// Frames decoded so far.
    pub fn ready(&self) -> usize {
        lock(&self.shared.frames).len()
    }

    pub fn is_complete(&self) -> bool {
        self.ready() == self.frame_count()
    }

    /// Decoding ended early, so nothing past `ready` is coming.
    pub fn is_stopped(&self) -> bool {
        self.shared.stopped.load(Ordering::Relaxed)
    }

    /// The frame on screen at `position_ms` of the loop.
    pub fn frame_at(&self, position_ms: i64) -> usize {
        self.timeline.frame_at(position_ms)
    }

    pub fn start_of(&self, frame: usize) -> i64 {
        self.timeline.start_of(frame as isize)
    }

    pub fn until_next_frame(&self, position_ms: i64) -> i64 {
        self.timeline.until_next_frame(position_ms)
    }

    pub fn wrap(&self, position_ms: i64) -> i64 {
        self.timeline.wrap(position_ms)
    }

    /// The frame asked for, or the last one decoded if it has not arrived yet, with the index it
    /// really is.
    pub fn frame_or_earlier(&self, frame: usize) -> (usize, Arc<Vec<u8>>) {
        let frames = lock(&self.shared.frames);
        let at = frame.min(frames.len().saturating_sub(1));
        (at, frames[at].clone())
    }

    pub fn close(&self) {
        self.shared.stopped.store(true, Ordering::Relaxed);
        self.shared.cancel.cancel();
    }
}

impl Drop for GifPlayer {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GIF89a with a 4-colour palette, `frames` full-canvas frames whose pixels are all index
    /// `n % 4`, uncompressed LZW (a clear code before every pixel).
    fn gif(width: u16, height: u16, frames: &[(u8, u16)]) -> Vec<u8> {
        let mut out = b"GIF89a".to_vec();
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&[0xF1, 0, 0]); // 4 entries
        out.extend_from_slice(&[0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
        for &(index, delay) in frames {
            out.extend_from_slice(&[0x21, 0xF9, 0x04, 0x04]);
            out.extend_from_slice(&delay.to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            out.push(0x2C);
            out.extend_from_slice(&[0, 0, 0, 0]);
            out.extend_from_slice(&width.to_le_bytes());
            out.extend_from_slice(&height.to_le_bytes());
            out.push(0);
            out.push(2); // min code size 2: clear = 4, end = 5, codes are 3 bits
            // 3-bit codes, packed little-endian: clear, pixel, ... end.
            let mut bits: Vec<u8> = Vec::new();
            let mut push = |code: u8| {
                for b in 0..3 {
                    bits.push((code >> b) & 1);
                }
            };
            for _ in 0..(width * height) {
                push(4);
                push(index);
            }
            push(5);
            let mut packed = Vec::new();
            for chunk in bits.chunks(8) {
                let mut byte = 0u8;
                for (i, b) in chunk.iter().enumerate() {
                    byte |= b << i;
                }
                packed.push(byte);
            }
            for block in packed.chunks(255) {
                out.push(block.len() as u8);
                out.extend_from_slice(block);
            }
            out.push(0);
        }
        out.push(0x3B);
        out
    }

    #[test]
    fn the_first_frame_is_there_and_the_rest_follow() {
        let player = GifPlayer::from_bytes(gif(2, 2, &[(1, 20), (2, 20), (3, 0)])).unwrap();
        assert_eq!(player.frame_count(), 3);
        assert_eq!((player.width, player.height), (2, 2));
        // 200 + 200 + 100 (a declared 0 plays at 100).
        assert_eq!(player.duration_ms(), 500);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !player.is_complete() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(player.is_complete());
        let (at, red) = player.frame_or_earlier(0);
        assert_eq!(at, 0);
        assert_eq!(&red[..4], &[0, 0, 255, 255]); // BGRA of palette entry 1 (red)
        assert_eq!(player.frame_at(250), 1);
        assert_eq!(player.frame_at(450), 2);
        assert_eq!(player.frame_at(550), 0); // loops
    }

    #[test]
    fn a_frame_not_yet_decoded_answers_with_the_last_one_there_is() {
        let player = GifPlayer::from_bytes(gif(2, 2, &[(1, 10), (2, 10)])).unwrap();
        let (at, _) = player.frame_or_earlier(99);
        assert!(at < 2);
    }

    #[test]
    fn one_frame_is_a_still_and_garbage_is_unreadable() {
        assert_eq!(
            GifPlayer::from_bytes(gif(2, 2, &[(1, 0)])).err(),
            Some(GifOpenError::Still)
        );
        assert_eq!(
            GifPlayer::from_bytes(b"not a gif".to_vec()).err(),
            Some(GifOpenError::Unreadable)
        );
    }
}
