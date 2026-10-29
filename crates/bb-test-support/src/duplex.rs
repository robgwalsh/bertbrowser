//! A blocking in-memory stream pair, so both halves of a pipe protocol can be driven without a
//! real pipe, and therefore without a real elevation prompt, which no test can ever answer. Port of
//! `DuplexPair` / `BlockingStreamBuffer` / `DuplexStream` in tests/BertBrowser.Core.Tests/PipeFakes.cs.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::{Arc, Condvar, Mutex};

use bb_core::elevation::ElevationStream;

/// One direction: writes append, reads block until there is something or it is closed.
struct Buffer {
    state: Mutex<BufferState>,
    ready: Condvar,
}

#[derive(Default)]
struct BufferState {
    bytes: VecDeque<u8>,
    closed: bool,
}

impl Buffer {
    fn new() -> Arc<Buffer> {
        Arc::new(Buffer {
            state: Mutex::new(BufferState::default()),
            ready: Condvar::new(),
        })
    }

    fn write(&self, data: &[u8]) -> io::Result<()> {
        let mut state = self.state.lock().unwrap();
        if state.closed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "The pipe has been closed.",
            ));
        }
        state.bytes.extend(data.iter().copied());
        self.ready.notify_all();
        Ok(())
    }

    /// Zero only once closed and drained, which is end-of-stream.
    fn read(&self, buf: &mut [u8]) -> usize {
        let mut state = self.state.lock().unwrap();
        while state.bytes.is_empty() && !state.closed {
            state = self.ready.wait(state).unwrap();
        }
        let mut read = 0;
        while read < buf.len() {
            match state.bytes.pop_front() {
                Some(byte) => {
                    buf[read] = byte;
                    read += 1;
                }
                None => break,
            }
        }
        read
    }

    fn close(&self) {
        let mut state = self.state.lock().unwrap();
        state.closed = true;
        self.ready.notify_all();
    }
}

/// Closing both directions is what a broken pipe looks like to the other end. Shared by every
/// split of one end, so the end closes when its last handle goes.
struct EndLifetime {
    incoming: Arc<Buffer>,
    outgoing: Arc<Buffer>,
}

impl Drop for EndLifetime {
    fn drop(&mut self) {
        self.incoming.close();
        self.outgoing.close();
    }
}

/// One end of a [`DuplexPair`]. Reads from one buffer, writes to the other.
pub struct DuplexEnd {
    incoming: Arc<Buffer>,
    outgoing: Arc<Buffer>,
    lifetime: Arc<EndLifetime>,
}

impl DuplexEnd {
    /// `Stream.Dispose()`: both directions closed at once.
    pub fn close(&self) {
        self.incoming.close();
        self.outgoing.close();
    }
}

/// A second handle on the same end, sharing its lifetime: the end closes when the last one goes.
impl Clone for DuplexEnd {
    fn clone(&self) -> DuplexEnd {
        DuplexEnd {
            incoming: self.incoming.clone(),
            outgoing: self.outgoing.clone(),
            lifetime: self.lifetime.clone(),
        }
    }
}

impl Read for DuplexEnd {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        Ok(self.incoming.read(buf))
    }
}

impl Write for DuplexEnd {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.outgoing.write(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl ElevationStream for DuplexEnd {
    fn split(&self) -> io::Result<Box<dyn ElevationStream>> {
        Ok(Box::new(self.clone()))
    }

    fn shutdown(&self) {
        self.close();
    }
}

pub struct DuplexPair;

impl DuplexPair {
    /// Two connected ends: what one writes, the other reads.
    pub fn create() -> (DuplexEnd, DuplexEnd) {
        let left_to_right = Buffer::new();
        let right_to_left = Buffer::new();
        let left = DuplexEnd {
            incoming: right_to_left.clone(),
            outgoing: left_to_right.clone(),
            lifetime: Arc::new(EndLifetime {
                incoming: right_to_left.clone(),
                outgoing: left_to_right.clone(),
            }),
        };
        let right = DuplexEnd {
            incoming: left_to_right.clone(),
            outgoing: right_to_left.clone(),
            lifetime: Arc::new(EndLifetime {
                incoming: left_to_right,
                outgoing: right_to_left,
            }),
        };
        (left, right)
    }
}
