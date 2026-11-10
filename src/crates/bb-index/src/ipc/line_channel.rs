//! The framing both of this app's pipes use: one UTF-8 line per message, `\n`-terminated,
//! bounded. Spec: Core/Ipc/LineChannel.cs.
//!
//! Shared rather than duplicated because the bound is the point. A peer that connects and then
//! streams without ever sending a newline must be cut off rather than allowed to grow a buffer until
//! the process dies, and that rule is worth having in exactly one place: the single-instance pipe and
//! the index-helper pipe both face a peer they did not write.
//!
//! Reading is a `LineReader` rather than a free function, and that is load-bearing for the index
//! protocol. A read fills a buffer, and whatever arrived after the newline belongs to the *next*
//! message; a read-one-line function has nowhere to keep it and silently drops it. The
//! single-instance pipe never noticed because it reads exactly one line per connection, but the
//! index helper pushes many down one stream, and there the lost bytes are lost messages.
//!
//! Generic over `Read`/`Write` so the tests run over a slice; in the processes it is a
//! `std::fs::File` over the pipe handle.

use std::io::{self, Read, Write};

/// Writes one line and flushes, so the peer sees it without waiting on a buffer. One write for the
/// whole line, so a message-mode pipe would still deliver it as one message.
pub fn write_line<W: Write + ?Sized>(stream: &mut W, line: &str) -> io::Result<()> {
    let mut payload = Vec::with_capacity(line.len() + 1);
    payload.extend_from_slice(line.as_bytes());
    payload.push(b'\n');
    stream.write_all(&payload)?;
    stream.flush()
}

/// Reads a single bounded line and discards anything buffered after it. Only correct where the
/// peer sends one message per stream, as the single-instance pipe does.
pub fn read_line<R: Read>(stream: &mut R, max_length: usize) -> io::Result<Option<String>> {
    LineReader::new(stream, max_length).read_line()
}

/// Reads `\n`-terminated UTF-8 lines from a stream, keeping whatever it buffered past the end of
/// one line for the next.
///
/// The newline scan is byte-wise and safe for multi-byte text without decoding as it goes: every
/// continuation byte of a UTF-8 sequence has its high bit set, so `0x0A` can only ever appear as a
/// real newline and never inside a character. The accumulated bytes are decoded once, at the end of
/// the line, so a character split across two reads still lands whole.
pub struct LineReader<R: Read> {
    stream: R,
    max_length: usize,
    buffer: [u8; 1024],
    start: usize,
    end: usize,
}

impl<R: Read> LineReader<R> {
    pub fn new(stream: R, max_length: usize) -> LineReader<R> {
        LineReader {
            stream,
            max_length,
            buffer: [0; 1024],
            start: 0,
            end: 0,
        }
    }

    /// The next line, or None at end of stream with nothing buffered. A stream that ends without a
    /// newline yields what arrived, since that may still be a whole message. A peer that never
    /// sends one is cut off at the cap. An error is the stream's own (a broken pipe), never a
    /// malformed line: invalid UTF-8 decodes lossily and is the protocol's to refuse.
    pub fn read_line(&mut self) -> io::Result<Option<String>> {
        let mut line: Vec<u8> = Vec::new();

        loop {
            while self.start < self.end {
                let byte = self.buffer[self.start];
                if byte == b'\n' {
                    self.start += 1;
                    return Ok(Some(decode(line)));
                }

                // Cap enforced per byte, not per read: otherwise a single 1 KB fill could carry
                // the buffer well past the limit before anything checked it.
                if line.len() >= self.max_length {
                    return Ok(Some(decode(line)));
                }

                line.push(byte);
                self.start += 1;
            }

            self.start = 0;
            self.end = self.stream.read(&mut self.buffer)?;
            if self.end == 0 {
                return Ok(if line.is_empty() {
                    None
                } else {
                    Some(decode(line))
                });
            }
        }
    }

    /// The stream back, with whatever is still buffered lost; for handing a pipe on once the
    /// session is over.
    pub fn into_inner(self) -> R {
        self.stream
    }
}

fn decode(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }
}
