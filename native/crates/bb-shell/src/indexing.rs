//! The app's side of the index helper: the elevated launcher and the listening pipe.
//! Spec: App/Services/Indexing/ElevatedIndexHostLauncher.cs, NamedPipeIndexTransport.cs,
//! WindowsIndexerPresence (in `bb_core::mft`).
//!
//! **The helper outlives the app.** Closing a window ends a session, not the helper, which is why
//! reopening costs no UAC prompt and no rebuild. So the endpoint is created once and held for the
//! app's life (the name is well-known per user, which is what lets a helper started under an earlier
//! app find this one), nothing here ever sends `Shutdown`, and **the app never launches the helper
//! at startup**: it attaches, and a banner offers the prompt.

use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bb_core::elevation::elevated_process::{self, ProcessHandle, StartError};
use bb_core::ipc::IndexEndpoint;
use bb_core::mft::{
    IndexHostLaunchResult, IndexHostLauncher, IndexStream, IndexTransport, IndexTransportFactory,
    IndexerPresenceProbe, MftIndexClient, WindowsIndexerPresence,
};

use crate::pipe::{PeerCheck, PipeCreateError, PipeServer, PipeSession};

/// The helper's file name, beside the app.
pub const INDEXER_EXECUTABLE: &str = "bb-indexer.exe";

/// Real buffers: see `crate::pipe`. They only need to hold a short burst of one-line messages.
const IN_BUFFER: u32 = 16 * 1024;
const OUT_BUFFER: u32 = 4 * 1024;

/// A file beside the running executable, never found by name on `PATH`: what is being launched is a
/// thing that gets an administrator token.
pub fn beside_this_executable(file_name: &str) -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(file_name)))
        .unwrap_or_else(|| file_name.into())
        .to_string_lossy()
        .into_owned()
}

/// Starts `bb-indexer.exe` with an administrator token. The helper is told where the database is
/// and nothing else: the pipe it calls back on is derived from its own token.
pub struct ElevatedIndexHostLauncher {
    helper_path: String,
    data_dir: String,
    process: Mutex<Option<(u32, ProcessHandle)>>,
}

impl ElevatedIndexHostLauncher {
    pub fn new(data_dir: &str) -> ElevatedIndexHostLauncher {
        ElevatedIndexHostLauncher::at(&beside_this_executable(INDEXER_EXECUTABLE), data_dir)
    }

    pub fn at(helper_path: &str, data_dir: &str) -> ElevatedIndexHostLauncher {
        ElevatedIndexHostLauncher {
            helper_path: helper_path.to_string(),
            data_dir: data_dir.to_string(),
            process: Mutex::new(None),
        }
    }

    /// Quoted, because the data directory can contain spaces (it is under the user's profile).
    /// Nothing here comes from a file being browsed; the path is this app's own.
    pub fn arguments(data_dir: &str) -> String {
        format!("--data-dir \"{}\"", data_dir.trim_end_matches('\\'))
    }
}

impl IndexHostLauncher for ElevatedIndexHostLauncher {
    fn launch(&self) -> IndexHostLaunchResult {
        if !std::path::Path::new(&self.helper_path).is_file() {
            return IndexHostLaunchResult::failed("the index helper is missing");
        }
        match elevated_process::start_elevated(&self.helper_path, &Self::arguments(&self.data_dir))
        {
            Ok((handle, process_id)) => {
                *self.process.lock().unwrap() = Some((process_id, handle));
                IndexHostLaunchResult::started(process_id)
            }
            Err(StartError::Declined) => IndexHostLaunchResult::declined(),
            Err(StartError::Failed(detail)) => IndexHostLaunchResult::failed(&detail),
        }
    }

    fn can_elevate(&self) -> bool {
        elevated_process::can_elevate()
    }

    fn wait_for_exit(&self, process_id: u32, timeout: Duration) {
        let taken = {
            let mut process = self.process.lock().unwrap();
            match process.as_ref() {
                Some((id, _)) if *id == process_id => process.take(),
                _ => None,
            }
        };
        if let Some((_, handle)) = taken {
            handle.wait(timeout);
        }
    }
}

/// One session's view of the shared pipe: closing it hangs up on this helper and leaves the
/// endpoint listening for the next.
struct PipeIndexStream(PipeSession);

impl IndexStream for PipeIndexStream {
    fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }

    fn write_all(&self, bytes: &[u8]) -> io::Result<()> {
        self.0.write_all(bytes)
    }

    fn close(&self) {
        self.0.close();
    }
}

/// The listening end of the index pipe, in this (unprivileged) process.
pub struct PipeIndexTransport {
    server: PipeServer,
}

impl PipeIndexTransport {
    pub fn create() -> Result<PipeIndexTransport, PipeCreateError> {
        let sid = IndexEndpoint::current_user_sid()
            .map_err(|e| PipeCreateError::Failed(e.to_string()))?;
        let name =
            IndexEndpoint::for_user(&sid).map_err(|e| PipeCreateError::Failed(e.to_string()))?;
        Ok(PipeIndexTransport {
            server: PipeServer::create(&name, &sid, IN_BUFFER, OUT_BUFFER)?,
        })
    }
}

impl IndexTransport for PipeIndexTransport {
    fn endpoint(&self) -> String {
        self.server.name().to_string()
    }

    fn accept(
        &self,
        launched_process_id: Option<u32>,
        timeout: Duration,
    ) -> Option<Arc<dyn IndexStream>> {
        let check = match launched_process_id {
            Some(id) => PeerCheck::Launched(id),
            None => PeerCheck::Elevated,
        };
        let session = self.server.accept(check, timeout)?;
        // Never the server itself: the caller closes the stream when a session ends, and the server
        // has to survive that to host the next one.
        Some(Arc::new(PipeIndexStream(session)))
    }
}

/// Holds the one endpoint for the app's life: a single instance, not one per attempt, because the
/// name is well-known and a second server would simply fail to be created, and the endpoint has to
/// stay up between attempts so a helper started after a failed look can still find it.
#[derive(Default)]
pub struct PipeIndexTransportFactory {
    transport: Mutex<Option<Arc<PipeIndexTransport>>>,
}

impl IndexTransportFactory for PipeIndexTransportFactory {
    fn try_create(&self) -> Result<Arc<dyn IndexTransport>, String> {
        let mut held = self.transport.lock().unwrap();
        if let Some(transport) = held.as_ref() {
            return Ok(transport.clone());
        }
        match PipeIndexTransport::create() {
            Ok(transport) => {
                let transport = Arc::new(transport);
                *held = Some(transport.clone());
                Ok(transport)
            }
            // The name is taken, which for a per-user name means another copy of this app owns the
            // index. Reported rather than thrown: it is an ordinary state, not a bug.
            Err(PipeCreateError::InUse) => {
                Err("another copy of BertBrowser is using it".to_string())
            }
            Err(PipeCreateError::Failed(why)) => Err(why),
        }
    }
}

/// The real index client: the elevated launcher, the held endpoint and the presence probe.
pub fn real_index_client(data_dir: &str) -> MftIndexClient {
    MftIndexClient::new(
        Arc::new(ElevatedIndexHostLauncher::new(data_dir)),
        Arc::new(PipeIndexTransportFactory::default()),
        Some(Arc::new(WindowsIndexerPresence) as Arc<dyn IndexerPresenceProbe>),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_helper_is_told_the_data_directory_and_nothing_else() {
        assert_eq!(
            r#"--data-dir "C:\Users\A B\.bertbrowser""#,
            ElevatedIndexHostLauncher::arguments(r"C:\Users\A B\.bertbrowser\")
        );
    }

    #[test]
    fn a_missing_helper_fails_before_any_prompt() {
        let launcher = ElevatedIndexHostLauncher::at(r"C:\no\such\bb-indexer.exe", r"C:\d");
        let result = launcher.launch();
        assert_eq!(bb_core::mft::IndexHostLaunch::Failed, result.outcome);
        assert_eq!("the index helper is missing", result.detail);
    }

    #[test]
    fn waiting_for_a_process_that_was_never_launched_returns_at_once() {
        let launcher = ElevatedIndexHostLauncher::at("x", "y");
        let start = std::time::Instant::now();
        launcher.wait_for_exit(4242, Duration::from_secs(5));
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn the_helper_is_found_beside_the_running_executable_never_by_name() {
        let path = beside_this_executable(INDEXER_EXECUTABLE);
        assert!(path.ends_with(INDEXER_EXECUTABLE));
        assert!(std::path::Path::new(&path).is_absolute());
    }

    #[test]
    fn the_transport_serves_one_helper_then_the_next_over_one_endpoint() {
        // The real per-user name: skip if a real app or helper is using it on this machine.
        let factory = PipeIndexTransportFactory::default();
        let Ok(transport) = factory.try_create() else {
            return;
        };
        assert_eq!(
            transport.endpoint(),
            factory.try_create().unwrap().endpoint()
        );
        // Nobody is dialling: a short accept times out and the endpoint stays up.
        assert!(
            transport
                .accept(Some(std::process::id()), Duration::from_millis(20))
                .is_none()
        );
        assert!(transport.accept(None, Duration::from_millis(20)).is_none());
    }
}
