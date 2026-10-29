//! A small GIF decoder: the header, the palettes, each frame's rectangle, delay and disposal, and
//! the LZW stream behind it. What WPF's `GifBitmapDecoder` gave the C# compositor, done here so
//! the preview needs no imaging stack to animate, and so the frames can be tested as bytes.
//!
//! Untrusted input throughout: nothing here indexes without a bounds check, a truncated stream
//! yields the frames it had, and a bad LZW code ends that frame's pixels rather than the decode.
//! Pixels come out BGRA32, a GIF pixel being fully opaque or fully transparent, which is what the
//! compositor expects.

use super::{GifDisposal, GifSubFrame};
use crate::cancel::{CancelToken, Cancelled};

/// What the header says, before any pixel is decoded: enough for `AnimationLimits::allows`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GifHeader {
    pub width: u32,
    pub height: u32,
    pub frame_count: usize,
}

/// A decoded animation: every sub-frame and its delay in milliseconds, one per frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GifAnimation {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<GifSubFrame>,
    pub delays_ms: Vec<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GifError {
    /// No `GIF87a`/`GIF89a` signature.
    NotAGif,
    /// The logical screen descriptor is missing or the canvas is empty.
    BadHeader,
    Cancelled,
}

impl From<Cancelled> for GifError {
    fn from(_: Cancelled) -> Self {
        GifError::Cancelled
    }
}

impl std::fmt::Display for GifError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            GifError::NotAGif => "not a GIF",
            GifError::BadHeader => "GIF header is unreadable",
            GifError::Cancelled => "cancelled",
        })
    }
}

impl std::error::Error for GifError {}

pub struct GifDecoder;

impl GifDecoder {
    /// The canvas size and the frame count, walking the block structure without decoding a pixel.
    pub fn probe(bytes: &[u8]) -> Result<GifHeader, GifError> {
        let mut reader = Reader::new(bytes)?;
        let (width, height) = (reader.width, reader.height);
        let mut frame_count = 0;
        while let Some(block) = reader.next_block() {
            if let Block::Image { .. } = block {
                frame_count += 1;
            }
        }
        Ok(GifHeader {
            width,
            height,
            frame_count,
        })
    }

    /// The header and every frame's declared delay in milliseconds, walking the block structure
    /// without decoding a pixel: the seek bar needs the whole length before the last frame exists.
    pub fn scan(bytes: &[u8]) -> Result<(GifHeader, Vec<i32>), GifError> {
        let mut reader = Reader::new(bytes)?;
        let (width, height) = (reader.width, reader.height);
        let mut delays = Vec::new();
        let mut control = GraphicControl::default();
        while let Some(block) = reader.next_block() {
            match block {
                Block::Control(c) => control = c,
                Block::Image { .. } => {
                    delays.push(i32::from(control.delay_cs) * 10);
                    control = GraphicControl::default();
                }
            }
        }
        Ok((
            GifHeader {
                width,
                height,
                frame_count: delays.len(),
            },
            delays,
        ))
    }

    /// Every frame, decoded. A stream that ends early yields the frames before the cut.
    pub fn decode(bytes: &[u8], cancel: &CancelToken) -> Result<GifAnimation, GifError> {
        let mut frames = Vec::new();
        let mut delays_ms = Vec::new();
        let (width, height) = Self::decode_each(bytes, cancel, &mut |frame, delay| {
            frames.push(frame);
            delays_ms.push(delay);
            true
        })?;
        Ok(GifAnimation {
            width,
            height,
            frames,
            delays_ms,
        })
    }

    /// Decodes frame by frame, handing each to `each` with its delay as soon as it exists, so a
    /// caller can show the first picture while the rest are still being read. `each` returning
    /// false stops the walk. Returns the canvas size.
    pub fn decode_each(
        bytes: &[u8],
        cancel: &CancelToken,
        each: &mut dyn FnMut(GifSubFrame, i32) -> bool,
    ) -> Result<(u32, u32), GifError> {
        let mut reader = Reader::new(bytes)?;
        let (width, height) = (reader.width, reader.height);
        let mut control = GraphicControl::default();

        while let Some(block) = reader.next_block() {
            cancel.check()?;
            match block {
                Block::Control(c) => control = c,
                Block::Image {
                    left,
                    top,
                    width,
                    height,
                    palette,
                    interlaced,
                    min_code_size,
                    data,
                } => {
                    let palette = palette.or(reader.global_palette);
                    let pixels = Self::pixels(
                        width,
                        height,
                        palette,
                        interlaced,
                        min_code_size,
                        &data,
                        control.transparent_index,
                    );
                    let frame = GifSubFrame {
                        left: i32::from(left),
                        top: i32::from(top),
                        width: i32::from(width),
                        height: i32::from(height),
                        pixels,
                        disposal: control.disposal,
                    };
                    let delay = i32::from(control.delay_cs) * 10;
                    // A graphic control extension governs the one image that follows it.
                    control = GraphicControl::default();
                    if !each(frame, delay) {
                        break;
                    }
                }
            }
        }
        Ok((width, height))
    }

    /// The frame's BGRA32 pixels. An index the LZW stream never produced stays transparent.
    fn pixels(
        width: u16,
        height: u16,
        palette: Option<&[u8]>,
        interlaced: bool,
        min_code_size: u8,
        data: &[u8],
        transparent_index: Option<u8>,
    ) -> Vec<u8> {
        let count = usize::from(width) * usize::from(height);
        let indices = lzw_decode(min_code_size, data, count);
        let mut pixels = vec![0u8; count * 4];

        let rows_in_order: Vec<usize> = if interlaced {
            interlace_order(usize::from(height))
        } else {
            (0..usize::from(height)).collect()
        };

        let width = usize::from(width);
        for (decoded_row, &row) in rows_in_order.iter().enumerate() {
            let src_start = decoded_row * width;
            if src_start >= indices.len() {
                break;
            }
            let src_end = (src_start + width).min(indices.len());
            for (x, &index) in indices[src_start..src_end].iter().enumerate() {
                if Some(index) == transparent_index {
                    continue;
                }
                let at = (row * width + x) * 4;
                let (r, g, b) = palette_entry(palette, index);
                pixels[at] = b;
                pixels[at + 1] = g;
                pixels[at + 2] = r;
                pixels[at + 3] = 255;
            }
        }
        pixels
    }
}

/// An index past the palette's end is black, as most decoders draw it.
fn palette_entry(palette: Option<&[u8]>, index: u8) -> (u8, u8, u8) {
    let at = usize::from(index) * 3;
    match palette {
        Some(p) if at + 2 < p.len() => (p[at], p[at + 1], p[at + 2]),
        _ => (0, 0, 0),
    }
}

/// The row order of an interlaced image: every eighth row from 0, then from 4, every fourth from
/// 2, every second from 1.
fn interlace_order(height: usize) -> Vec<usize> {
    let mut order = Vec::with_capacity(height);
    for (start, step) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
        order.extend((start..height).step_by(step));
    }
    order
}

#[derive(Clone, Copy, Default)]
struct GraphicControl {
    disposal: GifDisposal,
    delay_cs: u16,
    transparent_index: Option<u8>,
}

enum Block<'a> {
    Control(GraphicControl),
    Image {
        left: u16,
        top: u16,
        width: u16,
        height: u16,
        palette: Option<&'a [u8]>,
        interlaced: bool,
        min_code_size: u8,
        /// The LZW stream with the sub-block lengths removed.
        data: Vec<u8>,
    },
}

/// A cursor over the file's blocks. Every read is bounds-checked; running out of bytes ends the
/// walk rather than failing it.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    width: u32,
    height: u32,
    global_palette: Option<&'a [u8]>,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Result<Reader<'a>, GifError> {
        if !(bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
            return Err(GifError::NotAGif);
        }
        if bytes.len() < 13 {
            return Err(GifError::BadHeader);
        }
        let width = u32::from(u16::from_le_bytes([bytes[6], bytes[7]]));
        let height = u32::from(u16::from_le_bytes([bytes[8], bytes[9]]));
        if width == 0 || height == 0 {
            return Err(GifError::BadHeader);
        }
        let packed = bytes[10];
        let mut reader = Reader {
            bytes,
            at: 13,
            width,
            height,
            global_palette: None,
        };
        if packed & 0x80 != 0 {
            reader.global_palette = reader.take_palette(packed);
        }
        Ok(reader)
    }

    fn take_palette(&mut self, packed: u8) -> Option<&'a [u8]> {
        let entries = 2usize << (packed & 0x07);
        let palette = self.take(entries * 3)?;
        Some(palette)
    }

    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(count)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    fn byte(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }

    /// Skips the data sub-blocks of an extension, or collects an image's.
    fn sub_blocks(&mut self, collect: bool) -> Option<Vec<u8>> {
        let mut data = Vec::new();
        loop {
            let length = usize::from(self.byte()?);
            if length == 0 {
                return Some(data);
            }
            let block = self.take(length)?;
            if collect {
                data.extend_from_slice(block);
            }
        }
    }

    fn next_block(&mut self) -> Option<Block<'a>> {
        loop {
            match self.byte()? {
                0x3B => return None, // trailer
                0x21 => {
                    let label = self.byte()?;
                    if label == 0xF9 {
                        let _size = self.byte()?;
                        let packed = self.byte()?;
                        let delay_cs = self.u16()?;
                        let transparent = self.byte()?;
                        self.sub_blocks(false)?;
                        return Some(Block::Control(GraphicControl {
                            disposal: match (packed >> 2) & 0x07 {
                                2 => GifDisposal::Clear,
                                3 => GifDisposal::Restore,
                                _ => GifDisposal::Keep,
                            },
                            delay_cs,
                            transparent_index: (packed & 0x01 != 0).then_some(transparent),
                        }));
                    }
                    self.sub_blocks(false)?;
                }
                0x2C => {
                    let left = self.u16()?;
                    let top = self.u16()?;
                    let width = self.u16()?;
                    let height = self.u16()?;
                    let packed = self.byte()?;
                    let palette = if packed & 0x80 != 0 {
                        Some(self.take_palette(packed)?)
                    } else {
                        None
                    };
                    let min_code_size = self.byte()?;
                    let data = self.sub_blocks(true)?;
                    return Some(Block::Image {
                        left,
                        top,
                        width,
                        height,
                        palette,
                        interlaced: packed & 0x40 != 0,
                        min_code_size,
                        data,
                    });
                }
                // Anything else is not a block the format defines; the walk stops rather than
                // guessing where the next one starts.
                _ => return None,
            }
        }
    }
}

/// GIF's LZW: codes start one bit wider than the minimum, grow to twelve bits, and the clear code
/// resets the table. Stops at the end-of-information code, at `expected` pixels, at the end of
/// the data, or at a code the table cannot explain.
fn lzw_decode(min_code_size: u8, data: &[u8], expected: usize) -> Vec<u8> {
    let min_code_size = min_code_size.clamp(2, 11) as usize;
    let clear = 1usize << min_code_size;
    let end = clear + 1;

    // Each table entry is (prefix code, last byte, length); the string is rebuilt by walking
    // prefixes, which costs nothing per entry and never holds a copy of every string.
    let mut prefix: Vec<u16> = Vec::with_capacity(4096);
    let mut suffix: Vec<u8> = Vec::with_capacity(4096);
    let mut length: Vec<u16> = Vec::with_capacity(4096);
    let reset = |prefix: &mut Vec<u16>, suffix: &mut Vec<u8>, length: &mut Vec<u16>| {
        prefix.clear();
        suffix.clear();
        length.clear();
        for i in 0..clear + 2 {
            prefix.push(u16::MAX);
            suffix.push(i as u8);
            length.push(1);
        }
    };
    reset(&mut prefix, &mut suffix, &mut length);

    let mut out = Vec::with_capacity(expected);
    let mut code_size = min_code_size + 1;
    let mut previous: Option<usize> = None;
    let mut bit_buffer: u32 = 0;
    let mut bits = 0;
    let mut pos = 0;
    let mut scratch = Vec::new();

    while out.len() < expected {
        while bits < code_size {
            let Some(&b) = data.get(pos) else {
                return out;
            };
            pos += 1;
            bit_buffer |= u32::from(b) << bits;
            bits += 8;
        }
        let code = (bit_buffer & ((1 << code_size) - 1)) as usize;
        bit_buffer >>= code_size;
        bits -= code_size;

        if code == clear {
            reset(&mut prefix, &mut suffix, &mut length);
            code_size = min_code_size + 1;
            previous = None;
            continue;
        }
        if code == end {
            return out;
        }

        let first_byte_of = |code: usize, prefix: &[u16], suffix: &[u8]| {
            let mut c = code;
            while prefix[c] != u16::MAX {
                c = usize::from(prefix[c]);
            }
            suffix[c]
        };

        let entry = if code < prefix.len() {
            code
        } else if code == prefix.len() {
            // The one code that is not yet in the table: the previous string plus its own first
            // byte, which only makes sense when there was a previous string.
            match previous {
                Some(p) => {
                    let first = first_byte_of(p, &prefix, &suffix);
                    prefix.push(p as u16);
                    suffix.push(first);
                    length.push(length[p] + 1);
                    code
                }
                None => return out,
            }
        } else {
            return out;
        };

        // Emit the string for `entry`, written backwards through the prefix chain.
        scratch.clear();
        let mut c = entry;
        loop {
            scratch.push(suffix[c]);
            if prefix[c] == u16::MAX {
                break;
            }
            c = usize::from(prefix[c]);
        }
        let room = expected - out.len();
        out.extend(scratch.iter().rev().take(room));

        if let Some(p) = previous
            && entry != prefix.len() - 1
            && prefix.len() < 4096
        {
            // Add previous + first byte of this string.
            prefix.push(p as u16);
            suffix.push(*scratch.last().unwrap_or(&0));
            length.push(length[p] + 1);
        }
        if prefix.len() == (1 << code_size) && code_size < 12 {
            code_size += 1;
        }
        previous = Some(entry);
    }
    out
}
