//! Turns a GIF's frames (each only the rectangle that changed) into whole pictures, one per frame,
//! so that showing any frame is a lookup rather than a replay from the first.
//!
//! Fed one frame at a time (`next`) so the preview can show the first frame, and start playing,
//! while the rest are still being decoded. Pure byte arithmetic with no imaging dependency, so the
//! disposal rules are tested here rather than trusted to a decoder. A GIF is untrusted input: a
//! rectangle that runs off the canvas is clipped, and a pixel buffer shorter than its rectangle
//! claims is drawn as far as it goes. A GIF pixel is fully opaque or fully transparent, so drawing
//! is a copy wherever alpha is set. Spec: Core/Services/Preview/Animation/GifFrameCompositor.cs.

use crate::cancel::{CancelToken, Cancelled};

/// What a GIF frame asks to be done with its area once its delay is up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GifDisposal {
    /// 0 and 1 in the file: leave it where it is for the next frame to draw over.
    #[default]
    Keep,

    /// 2: clear its rectangle. To transparent, as every browser does, not to the "background
    /// colour" the spec names and nobody honours.
    Clear,

    /// 3: put back whatever was there before it was drawn.
    Restore,
}

/// One frame as the file stores it: a rectangle of BGRA32 pixels somewhere on the canvas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GifSubFrame {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    /// BGRA32, row-major, `width * height * 4` bytes when complete.
    pub pixels: Vec<u8>,
    pub disposal: GifDisposal,
}

pub struct GifFrameCompositor {
    width: usize,
    height: usize,
    canvas: Vec<u8>,

    /// The frame last drawn, whose disposal is still owed: carried out at the start of the next
    /// call, so the canvas stays that frame's picture for as long as a caller may be reading it.
    /// Only the rectangle and the disposal are kept, since that is all the disposal needs.
    undisposed: Option<(i32, i32, i32, i32, GifDisposal)>,

    /// What was under a `GifDisposal::Restore` frame's rectangle. Only the rectangle, and one
    /// buffer reused: saving the whole canvas per frame was a large allocation for every frame
    /// of the animation.
    under: Vec<u8>,
}

impl GifFrameCompositor {
    /// A canvas of `width` × `height`. Both must be positive: `AnimationLimits::allows` is the
    /// gate that keeps a zero-sized header from reaching here.
    pub fn new(width: u32, height: u32) -> GifFrameCompositor {
        assert!(width > 0 && height > 0, "a GIF canvas has a size");
        let (width, height) = (width as usize, height as usize);
        GifFrameCompositor {
            width,
            height,
            canvas: vec![0u8; width * height * 4],
            undisposed: None,
            under: Vec::new(),
        }
    }

    /// Draws the next frame and returns the whole picture it makes: a copy of its own, since the
    /// canvas goes on to be disposed of and drawn over.
    pub fn next(&mut self, frame: &GifSubFrame) -> Vec<u8> {
        self.next_shared(frame).to_vec()
    }

    /// Draws the next frame and returns the canvas itself, which is that frame's whole picture
    /// **only until the next call**. For a caller that copies the pixels somewhere of its own
    /// straight away (a bitmap does) and so has no use for a second copy in between.
    pub fn next_shared(&mut self, frame: &GifSubFrame) -> &[u8] {
        if let Some((left, top, width, height, disposal)) = self.undisposed {
            let rect = self.clip(left, top, width, height);
            match disposal {
                GifDisposal::Restore => self.restore_rect(rect),
                GifDisposal::Clear => self.clear_rect(rect),
                GifDisposal::Keep => {}
            }
        }

        let rect = self.clip(frame.left, frame.top, frame.width, frame.height);
        if frame.disposal == GifDisposal::Restore {
            self.save_rect(rect);
        }
        self.draw(frame, rect);
        self.undisposed = Some((
            frame.left,
            frame.top,
            frame.width,
            frame.height,
            frame.disposal,
        ));
        &self.canvas
    }

    /// Every frame at once.
    pub fn compose(
        width: u32,
        height: u32,
        frames: impl IntoIterator<Item = GifSubFrame>,
        cancel: &CancelToken,
    ) -> Result<Vec<Vec<u8>>, Cancelled> {
        let mut compositor = GifFrameCompositor::new(width, height);
        let mut composed = Vec::new();
        for frame in frames {
            cancel.check()?;
            composed.push(compositor.next(&frame));
        }
        Ok(composed)
    }

    /// Saves the frame's clipped rectangle out of the canvas.
    fn save_rect(&mut self, (x0, y0, x1, y1): Rect) {
        let row_bytes = (x1 - x0) * 4;
        let needed = row_bytes * (y1 - y0);
        if self.under.len() < needed {
            self.under.resize(needed, 0);
        }
        for y in y0..y1 {
            let canvas_at = (y * self.width + x0) * 4;
            let under_at = (y - y0) * row_bytes;
            self.under[under_at..under_at + row_bytes]
                .copy_from_slice(&self.canvas[canvas_at..canvas_at + row_bytes]);
        }
    }

    /// Puts a saved rectangle back.
    fn restore_rect(&mut self, (x0, y0, x1, y1): Rect) {
        let row_bytes = (x1 - x0) * 4;
        for y in y0..y1 {
            let canvas_at = (y * self.width + x0) * 4;
            let under_at = (y - y0) * row_bytes;
            self.canvas[canvas_at..canvas_at + row_bytes]
                .copy_from_slice(&self.under[under_at..under_at + row_bytes]);
        }
    }

    fn draw(&mut self, frame: &GifSubFrame, (x0, y0, x1, y1): Rect) {
        let frame_width = frame.width.max(0) as usize;
        for y in y0..y1 {
            for x in x0..x1 {
                let src = ((y as i64 - i64::from(frame.top)) as usize * frame_width
                    + (x as i64 - i64::from(frame.left)) as usize)
                    * 4;
                // Short buffer: drawn as far as it goes, and no further.
                if src + 3 >= frame.pixels.len() {
                    return;
                }
                if frame.pixels[src + 3] == 0 {
                    continue;
                }

                let dst = (y * self.width + x) * 4;
                self.canvas[dst..dst + 4].copy_from_slice(&frame.pixels[src..src + 4]);
            }
        }
    }

    fn clear_rect(&mut self, (x0, y0, x1, y1): Rect) {
        for y in y0..y1 {
            let at = (y * self.width + x0) * 4;
            self.canvas[at..at + (x1 - x0) * 4].fill(0);
        }
    }

    /// The frame's rectangle clipped to the canvas, as `(x0, y0, x1, y1)`.
    fn clip(&self, left: i32, top: i32, width: i32, height: i32) -> Rect {
        let x0 = i64::from(left).clamp(0, self.width as i64);
        let y0 = i64::from(top).clamp(0, self.height as i64);
        let x1 = (i64::from(left) + i64::from(width.max(0))).clamp(x0, self.width as i64);
        let y1 = (i64::from(top) + i64::from(height.max(0))).clamp(y0, self.height as i64);
        (x0 as usize, y0 as usize, x1 as usize, y1 as usize)
    }
}

type Rect = (usize, usize, usize, usize);
