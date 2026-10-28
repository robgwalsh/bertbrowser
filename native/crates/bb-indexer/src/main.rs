//! BertBrowser's index helper: the one process in the product that runs with an administrator
//! token, because reading the NTFS MFT needs a raw volume handle and nothing else does. Spec:
//! src/BertBrowser.Indexer/Program.cs.
//!
//! It takes five verbs (hello, start, shutdown, ping, record) and never receives a path, never
//! launches anything, and never draws a window. **It outlives the app**: a lost pipe ends a
//! *session*; this process waits for the next app. The three things that end it are an app asking
//! (`Shutdown`), the user signing out, and `self_checks`, which stop it when the app it belongs to
//! has been updated, removed, or has migrated the database out from under it. It is single-instance
//! (two would tail one journal into one database), enforced by the presence mutex.
//!
//! Two extra subcommands exist for the rewrite: `--selftest` runs the whole indexer over the
//! in-memory fake volume and needs no token; `--dump-stats` reads the real volumes and does.

mod autostart;
mod dump_stats;
mod pipe;
mod self_checks;
mod selftest;

use std::sync::Arc;
use std::time::Duration;

use bb_core::cancel::CancelToken;
use bb_core::data::{
    ChangeLogRepository, Db, DirSizeRepository, EXPECTED_SCHEMA_VERSION, FsIndexRepository,
};
use bb_core::ipc::{
    IndexEndpoint, IndexerArguments, IndexerCommand, IndexerPresenceLock, PipeOwner,
};
use bb_core::mft::{
    ChangeRecorderOptions, IndexSessionEnd, MftIndex, MftIndexHost, MftIndexService,
    RawVolumeAccess,
};
use bb_paths::PathKey;

/// How long to wait for an app between connection attempts: an explicit cadence rather than one
/// long blocking connect, so how quickly a starting app is picked up is a number chosen here. A
/// quarter of a second is imperceptible against a window opening.
const CONNECT_POLL_INTERVAL: Duration = Duration::from_millis(250);

fn main() {
    std::process::exit(run_main());
}

fn run_main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // The two diagnostics take no data directory and go nowhere near the shared parser, whose
    // "an unrecognised argument is an error" rule the tests pin.
    match args.first().map(String::as_str) {
        Some("--selftest") if args.len() == 1 => return selftest::run(),
        Some("--dump-stats") if args.len() == 1 => return dump_stats::run(),
        _ => {}
    }

    let options = match IndexerArguments::try_parse(&args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };

    match options.command {
        IndexerCommand::RegisterAutoStart => autostart::register(&options.data_directory),
        IndexerCommand::UnregisterAutoStart => autostart::unregister(),
        IndexerCommand::Run => run(&options),
    }
}

fn run(options: &IndexerArguments) -> i32 {
    let sid = match IndexEndpoint::current_user_sid() {
        Ok(sid) => sid,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };

    // One helper per user, or two would index the same volumes into the same database.
    let _claim = match IndexerPresenceLock::try_acquire(&sid) {
        Ok(Some(claim)) => claim,
        Ok(None) => {
            eprintln!("Another index helper is already running.");
            return 6;
        }
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };

    let db_path = std::path::Path::new(&options.data_directory).join("bertbrowser.db");
    if !db_path.is_file() {
        // The app creates and migrates the database under its own token, before it ever starts
        // this process. Creating one here would leave an Administrators-owned file behind, and at
        // sign-in, before the app has ever run, exiting quietly is the right answer.
        eprintln!("No database at the given data directory.");
        return 5;
    }

    // Never migrate: the schema belongs to the app, which has already applied it.
    let db = Arc::new(Db::attach(db_path));
    match db.schema_version() {
        Ok(version) if version == EXPECTED_SCHEMA_VERSION => {}
        Ok(_) => {
            // An app of a different build owns this database. Writing rows against a schema this
            // build does not know is worse than not indexing.
            eprintln!("The database is at a different schema version than this helper.");
            return 7;
        }
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    }

    // The change log's writer lives here too, because the records come from the same tail. Off
    // until the app says otherwise (Record), and it never records its own database's growth: the
    // data directory is what the exclusion is. It keeps whatever the last app pushed while none is
    // connected, so a session closing leaves no gap in the log.
    let excluded_root_key = match PathKey::canonicalize(&options.data_directory) {
        Ok(key) => key,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let changes = ChangeRecorderOptions {
        repository: Arc::new(ChangeLogRepository::new(db.clone())),
        excluded_root_key,
    };
    let index: Arc<MftIndexService> = Arc::new(MftIndexService::new(
        Arc::new(FsIndexRepository::new(db.clone())),
        Arc::new(DirSizeRepository::new(db.clone())),
        Arc::new(RawVolumeAccess),
        Some(changes),
    ));

    let lifetime = CancelToken::new();
    let _self_checks = self_checks::start(db.clone(), lifetime.clone());

    // Indexing is this process's job whether or not anyone is watching: started by the sign-in
    // task there may be no app for hours, and the whole point is that the index is warm when one
    // arrives.
    if let Err(error) = index.start() {
        eprintln!("{error}");
        return 1;
    }

    let host = MftIndexHost::new(index.clone());
    let code = serve(&host, &sid, &lifetime);
    index.shutdown();
    code
}

/// Hosts one app after another until asked to stop.
fn serve(host: &MftIndexHost, sid: &str, lifetime: &CancelToken) -> i32 {
    let endpoint = match IndexEndpoint::for_user(sid) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };

    while !lifetime.is_cancelled() {
        // A fresh connection per attempt; a pipe client cannot be reconnected. Overlapped, and
        // that is load-bearing: Windows serialises I/O on a non-overlapped handle, so this thread
        // parked in a blocking read would block the volume threads' writes on the same handle,
        // and not one progress message could leave while the app sat waiting for exactly those.
        let Some(client) = pipe::PipeClient::connect(&endpoint, CONNECT_POLL_INTERVAL) else {
            // No app is listening yet. Indexing carries on regardless.
            std::thread::sleep(CONNECT_POLL_INTERVAL);
            continue;
        };

        if !is_our_app(&client) {
            // Not the app this helper ships beside. Hang up rather than report to it.
            continue;
        }

        let stream: Arc<dyn bb_core::mft::IndexStream> = Arc::new(client);
        if host.run(stream, lifetime) == IndexSessionEnd::Stop {
            return 0;
        }
        // A broken session is the ordinary way an app goes away.
    }
    0
}

/// Whether the process listening on the pipe is the BertBrowser this helper ships beside. A
/// coherence check, not a boundary: nothing between two processes of one user is one. What this
/// rules out is confusion: a stale build, or another program that happened to claim the name. The
/// direction works because a high-integrity process may open a medium-integrity one for
/// `PROCESS_QUERY_LIMITED_INFORMATION`; the reverse would not.
fn is_our_app(client: &pipe::PipeClient) -> bool {
    let Some(server_process_id) = PipeOwner::server_process_id(client) else {
        return false;
    };
    let Some(actual) = PipeOwner::image_path_of(server_process_id) else {
        return false;
    };
    let Some(expected) = app_beside_us() else {
        return false;
    };
    matches!(
        (PathKey::canonicalize(&actual), PathKey::canonicalize(&expected)),
        (Ok(a), Ok(e)) if a == e
    )
}

/// `BertBrowser.exe` in this executable's directory.
fn app_beside_us() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    Some(dir.join("BertBrowser.exe").to_string_lossy().into_owned())
}
