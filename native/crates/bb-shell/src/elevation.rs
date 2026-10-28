//! The app's side of the file-operation helper: the pipe one elevated operation talks over, and the
//! runner the app calls when the user accepts the offer. Spec: App/Services/Elevation/*.
//!
//! The helper is one-shot: one `bb-elevator.exe` per operation Windows refused, started by a click
//! on a shield, serving one request and exiting. Nothing here runs at launch and nothing retries on
//! a timer, since every attempt is a UAC prompt.

use std::io::{self, Read, Write};
use std::time::Duration;

use bb_core::cancel::CancelToken;
use bb_core::delete::{DeleteOutcome, DeleteProgress, DeleteUndoResult};
use bb_core::elevation::{
    DeleteRetry, ElevatedOperationRunner, ElevatedRun, ElevationClient, ElevationStream,
    ElevationTransport, ElevationTransportFactory, NewItemRetry, RenameRetry, ShellExecuteLauncher,
    TransferRetry,
};
use bb_core::ipc::{ElevatorArguments, IndexEndpoint};
use bb_core::newitem::NewItemOutcome;
use bb_core::rename::RenameOutcome;
use bb_core::transfer::{TransferOutcome, TransferProgress, TransferUndoResult};

use crate::indexing::beside_this_executable;
use crate::pipe::{PeerCheck, PipeCreateError, PipeServer, PipeSession};

/// The helper's file name, beside the app.
pub const ELEVATOR_EXECUTABLE: &str = "bb-elevator.exe";

const IN_BUFFER: u32 = 16 * 1024;
const OUT_BUFFER: u32 = 16 * 1024;

/// A 128-bit nonce for the pipe name, as 32 hex digits. Pipe names are one machine-wide,
/// first-come namespace with no per-user partitioning, so a predictable name is one another
/// signed-in account could take first.
pub fn nonce() -> String {
    use windows_sys::Win32::Security::Cryptography::{
        BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
    };
    let mut bytes = [0u8; 16];
    // SAFETY: fills a 16-byte buffer from the system RNG.
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        // The RNG failing is not a state to carry on from with a guessable name.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        bytes.copy_from_slice(&t.to_le_bytes());
    }
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

struct ElevationPipeStream(PipeSession);

impl Read for ElevationPipeStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for ElevationPipeStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl ElevationStream for ElevationPipeStream {
    fn split(&self) -> io::Result<Box<dyn ElevationStream>> {
        Ok(Box::new(ElevationPipeStream(self.0.clone())))
    }

    fn shutdown(&self) {
        self.0.close();
    }
}

/// The pipe one elevated operation talks over. The app is the server and the helper the client.
pub struct PipeElevationTransport {
    endpoint: String,
    server: Option<PipeServer>,
}

impl PipeElevationTransport {
    pub fn create() -> Result<PipeElevationTransport, PipeCreateError> {
        let sid = IndexEndpoint::current_user_sid()
            .map_err(|e| PipeCreateError::Failed(e.to_string()))?;
        let endpoint = format!("{}{}", ElevatorArguments::PIPE_PREFIX, nonce());
        let server = PipeServer::create(&endpoint, &sid, IN_BUFFER, OUT_BUFFER)?;
        Ok(PipeElevationTransport {
            endpoint,
            server: Some(server),
        })
    }

    /// A transport whose pipe could not be created: it names an endpoint nobody can reach and never
    /// accepts, so the launch that follows ends in "the helper did not connect" rather than a crash.
    fn unusable() -> PipeElevationTransport {
        PipeElevationTransport {
            endpoint: format!("{}unavailable", ElevatorArguments::PIPE_PREFIX),
            server: None,
        }
    }
}

impl ElevationTransport for PipeElevationTransport {
    fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Waits for the helper to connect, then checks it is the one we started and running as us.
    /// Never a naked wait: a UAC prompt left sitting on screen would park the thread until answered.
    fn accept(&mut self, process_id: u32, timeout: Duration) -> Option<Box<dyn ElevationStream>> {
        let session = self
            .server
            .as_ref()?
            .accept(PeerCheck::Launched(process_id), timeout)?;
        Some(Box::new(ElevationPipeStream(session)))
    }
}

#[derive(Default)]
pub struct PipeElevationTransportFactory;

impl ElevationTransportFactory for PipeElevationTransportFactory {
    fn create(&self) -> Box<dyn ElevationTransport> {
        Box::new(
            PipeElevationTransport::create().unwrap_or_else(|_| PipeElevationTransport::unusable()),
        )
    }
}

/// Runs retries with an administrator token. A fresh `ElevationClient` per operation, because the
/// helper is one-shot and so is everything that talks to it; this object itself holds only the
/// helper's path and the user's SID, so it can be shared between threads.
pub struct SystemElevation {
    helper_path: String,
    user_sid: String,
}

impl SystemElevation {
    pub fn new() -> SystemElevation {
        SystemElevation::at(
            &beside_this_executable(ELEVATOR_EXECUTABLE),
            &IndexEndpoint::current_user_sid().unwrap_or_default(),
        )
    }

    pub fn at(helper_path: &str, user_sid: &str) -> SystemElevation {
        SystemElevation {
            helper_path: helper_path.to_string(),
            user_sid: user_sid.to_string(),
        }
    }

    fn client(&self) -> ElevationClient {
        ElevationClient::new(
            Box::new(ShellExecuteLauncher::new(&self.helper_path)),
            Box::new(PipeElevationTransportFactory),
            &self.user_sid,
        )
    }
}

impl Default for SystemElevation {
    fn default() -> Self {
        SystemElevation::new()
    }
}

impl ElevatedOperationRunner for SystemElevation {
    fn can_elevate(&self) -> bool {
        self.client().can_elevate()
    }

    fn run_transfer(
        &self,
        retry: &TransferRetry,
        progress: Option<&mut dyn FnMut(TransferProgress)>,
        cancel: &CancelToken,
    ) -> ElevatedRun<TransferOutcome> {
        self.client().run_transfer(retry, progress, cancel)
    }

    fn run_delete(
        &self,
        retry: &DeleteRetry,
        progress: Option<&mut dyn FnMut(DeleteProgress)>,
        cancel: &CancelToken,
    ) -> ElevatedRun<DeleteOutcome> {
        self.client().run_delete(retry, progress, cancel)
    }

    fn run_rename(&self, retry: &RenameRetry, cancel: &CancelToken) -> ElevatedRun<RenameOutcome> {
        self.client().run_rename(retry, cancel)
    }

    fn run_new_item(
        &self,
        retry: &NewItemRetry,
        cancel: &CancelToken,
    ) -> ElevatedRun<NewItemOutcome> {
        self.client().run_new_item(retry, cancel)
    }

    fn undo_transfer(
        &self,
        outcome: &TransferOutcome,
        cancel: &CancelToken,
    ) -> ElevatedRun<TransferUndoResult> {
        self.client().undo_transfer(outcome, cancel)
    }

    fn undo_delete(
        &self,
        outcome: &DeleteOutcome,
        cancel: &CancelToken,
    ) -> ElevatedRun<DeleteUndoResult> {
        self.client().undo_delete(outcome, cancel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb_core::elevation::ElevationStatus;
    use bb_core::ipc::IndexEndpoint;

    #[test]
    fn the_nonce_is_32_hex_digits_and_does_not_repeat() {
        let a = nonce();
        assert_eq!(32, a.len());
        assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(a, nonce());
    }

    #[test]
    fn the_pipe_name_is_one_the_helper_accepts() {
        let transport = PipeElevationTransport::create().unwrap();
        assert!(ElevatorArguments::is_acceptable_pipe_name(
            transport.endpoint()
        ));
    }

    #[test]
    fn a_helper_that_never_connects_ends_in_a_timeout_not_a_hang() {
        let mut transport = PipeElevationTransport::create().unwrap();
        assert!(
            transport
                .accept(std::process::id(), Duration::from_millis(30))
                .is_none()
        );
    }

    #[test]
    fn a_missing_helper_is_reported_before_any_prompt_is_raised() {
        // Only meaningful where this account could be asked: the launcher fails on the missing file
        // first, which is what keeps a UAC prompt out of this test either way.
        let runner = SystemElevation::at(
            r"C:\no\such\bb-elevator.exe",
            &IndexEndpoint::current_user_sid().unwrap(),
        );
        if !runner.can_elevate() {
            return;
        }
        let retry = NewItemRetry {
            plan: bb_core::newitem::NewItemPlan {
                directory: r"C:\x".to_string(),
                name: "a.txt".to_string(),
                ..bb_core::newitem::NewItemPlan::empty()
            },
        };
        let run = runner.run_new_item(&retry, &CancelToken::new());
        assert_eq!(ElevationStatus::Unavailable, run.status);
        assert!(run.detail.contains("missing"), "{}", run.detail);
    }
}
