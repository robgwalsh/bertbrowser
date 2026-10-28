// Port of tests/BertBrowser.Core.Tests/ElevationHostTests.cs
//
// The elevated end, driven over a `DuplexPair` against real files: everything the real helper does
// except holding a token, which is the one part no test can have.
//
// The tests that matter most are the refusals. This is the only class in the product that takes
// attacker-shaped input and acts on it with administrator rights, so "it does the operation" is the
// easy half; "it does nothing else, and only once" is the half worth pinning down.
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bb_core::cancel::CancelToken;
use bb_core::elevation::{
    ElevationHeader, ElevationHost, ElevationOperation, ElevationTransferItem,
};
use bb_core::ipc::{ElevationMessage, ElevationProtocol, ElevationVerb, LineReader, write_line};
use bb_core::newitem::{NewItemKind, NewItemPlan};
use bb_core::rename::PlannedRename;
use bb_core::transfer::{ConflictResolution, PlannedTransfer};
use bb_test_support::duplex::{DuplexEnd, DuplexPair};
use bb_test_support::unique_temp_path;

const PATIENCE: Duration = Duration::from_secs(5);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-elev", "");
        std::fs::create_dir_all(&root).unwrap();
        Fixture { root }
    }

    fn p(&self, parts: &[&str]) -> String {
        parts
            .iter()
            .fold(self.root.clone(), |p, part| p.join(part))
            .to_string_lossy()
            .into_owned()
    }

    fn dir(&self, parts: &[&str]) -> String {
        let path = self.p(parts);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn file(&self, content: &str, parts: &[&str]) -> String {
        let path = self.p(parts);
        std::fs::create_dir_all(Path::new(&path).parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }

    fn rename_item(&self, from: &str, to: &str) -> String {
        ElevationHost::write(&PlannedRename::new(&self.p(&[from]), &self.p(&[to]), false))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The host on its own thread over one end of a pair, and the app end to drive it with.
fn hosted() -> (JoinHandle<()>, ElevationPeer) {
    let (helper_side, app_side) = DuplexPair::create();
    let thread = std::thread::spawn(move || {
        ElevationHost::new(Box::new(helper_side)).run(&CancelToken::none());
    });
    (thread, ElevationPeer::new(app_side))
}

/// `Thread.Join(timeout)`: true when the host finished in time.
fn join(thread: &JoinHandle<()>, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while !thread.is_finished() {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

fn drain(app: &mut ElevationPeer, until: ElevationVerb) {
    while let Some(message) = app.receive() {
        if message.verb == until {
            break;
        }
    }
}

fn header(operation: ElevationOperation) -> String {
    ElevationHost::write(&ElevationHeader::new(operation))
}

fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

// --- the greeting ---

#[test]
fn it_greets_and_says_it_is_ready() {
    let (host, mut app) = hosted();

    assert_eq!(ElevationVerb::Hello, app.receive().unwrap().verb);
    assert_eq!(ElevationVerb::Ready, app.receive().unwrap().verb);

    app.close();
    join(&host, PATIENCE);
}

#[test]
fn a_different_protocol_version_ends_the_session() {
    let (host, mut app) = hosted();
    drain(&mut app, ElevationVerb::Ready);

    app.send(ElevationVerb::Hello, "99");

    assert_eq!(ElevationVerb::Fatal, app.receive().unwrap().verb);
    assert!(join(&host, PATIENCE));
}

#[test]
fn a_malformed_line_does_not_end_the_session() {
    let f = Fixture::new();
    let (host, mut app) = hosted();
    drain(&mut app, ElevationVerb::Ready);

    app.send_raw("this is not a message");
    app.request(
        &header(ElevationOperation::Rename),
        &[f.rename_item("a.txt", "b.txt")],
    );

    assert!(
        app.receive_all()
            .iter()
            .any(|m| m.verb == ElevationVerb::End)
    );
    assert!(join(&host, PATIENCE));
}

// --- one request, and only one ---

#[test]
fn it_serves_one_request_and_then_stops() {
    let f = Fixture::new();
    let (host, mut app) = hosted();
    drain(&mut app, ElevationVerb::Ready);
    f.file("first", &["a.txt"]);
    f.file("second", &["c.txt"]);

    app.request(
        &header(ElevationOperation::Rename),
        &[f.rename_item("a.txt", "b.txt")],
    );
    // A second request, sent the moment the first is accepted. The helper is one-shot by
    // construction, so this must reach nothing at all.
    app.request(
        &header(ElevationOperation::Rename),
        &[f.rename_item("c.txt", "d.txt")],
    );

    app.receive_all();
    assert!(join(&host, PATIENCE));

    assert!(exists(&f.p(&["b.txt"])));
    assert!(exists(&f.p(&["c.txt"])));
    assert!(!exists(&f.p(&["d.txt"])));
}

#[test]
fn an_item_arriving_after_go_is_not_accepted_into_the_request() {
    // The structural half of "one prompt, one plan": a peer that somehow owned the pipe still
    // cannot add a path to an operation that is already running.
    let f = Fixture::new();
    let (host, mut app) = hosted();
    drain(&mut app, ElevationVerb::Ready);
    f.file("one", &["a.txt"]);
    f.file("two", &["c.txt"]);

    app.request(
        &header(ElevationOperation::Rename),
        &[f.rename_item("a.txt", "b.txt")],
    );
    app.send(ElevationVerb::Item, &f.rename_item("c.txt", "d.txt"));

    app.receive_all();
    join(&host, PATIENCE);

    assert!(exists(&f.p(&["c.txt"])));
    assert!(!exists(&f.p(&["d.txt"])));
}

#[test]
fn a_second_header_is_refused() {
    // The state machine's half of "one request per process", asserted directly rather than
    // through its effect on disk: after Go the pipe is closed anyway, so a test that only looked
    // at the filesystem would pass whatever this switch did.
    let (host, mut app) = hosted();
    drain(&mut app, ElevationVerb::Ready);

    app.send(ElevationVerb::Begin, &header(ElevationOperation::Rename));
    app.send(ElevationVerb::Begin, &header(ElevationOperation::Delete));

    // Join first, and deliberately: a helper that accepted the second header would sit waiting
    // for items that are never coming, and reading before joining would wait with it forever
    // rather than failing.
    assert!(
        join(&host, PATIENCE),
        "a second header should end the session."
    );
    assert_eq!(ElevationVerb::Fatal, app.receive().unwrap().verb);
}

#[test]
fn an_item_before_a_header_is_refused() {
    let f = Fixture::new();
    let (host, mut app) = hosted();
    drain(&mut app, ElevationVerb::Ready);

    app.send(ElevationVerb::Item, &f.rename_item("a.txt", "b.txt"));

    assert_eq!(ElevationVerb::Fatal, app.receive().unwrap().verb);
    assert!(join(&host, PATIENCE));
}

#[test]
fn more_items_than_the_cap_refuses_the_whole_request() {
    let f = Fixture::new();
    let (host, mut app) = hosted();
    drain(&mut app, ElevationVerb::Ready);
    let victim = f.file("here", &["a.txt"]);

    app.send(
        ElevationVerb::Hello,
        &ElevationProtocol::PROTOCOL_VERSION.to_string(),
    );
    app.send(ElevationVerb::Begin, &header(ElevationOperation::Rename));
    let item = f.rename_item("a.txt", "b.txt");
    for _ in 0..=ElevationProtocol::MAX_ITEMS {
        app.send(ElevationVerb::Item, &item);
    }

    assert_eq!(ElevationVerb::Fatal, app.receive().unwrap().verb);
    assert!(join(&host, PATIENCE));
    assert!(exists(&victim));
}

// --- what it refuses to touch ---

#[test]
fn a_path_the_ipc_rule_refuses_is_refused_whole() {
    for source in [
        r"..\outside.txt",
        r"\\.\PhysicalDrive0",
        r"\\?\C:\Windows\evil.txt",
        r"C:\a*.txt",
    ] {
        let f = Fixture::new();
        let (host, mut app) = hosted();
        drain(&mut app, ElevationVerb::Ready);
        let bystander = f.file("untouched", &["a.txt"]);

        app.request(
            &header(ElevationOperation::Rename),
            &[
                ElevationHost::write(&PlannedRename::new(source, &f.p(&["b.txt"]), false)),
                f.rename_item("a.txt", "c.txt"),
            ],
        );

        assert!(
            app.receive_all()
                .iter()
                .any(|m| m.verb == ElevationVerb::Fatal),
            "{source}"
        );
        assert!(join(&host, PATIENCE), "{source}");

        // Refused whole, not in part: the good item in the same request was not carried out either.
        assert!(exists(&bystander), "{source}");
        assert!(!exists(&f.p(&["c.txt"])), "{source}");
    }
}

#[test]
fn an_unreadable_item_refuses_the_whole_request() {
    let f = Fixture::new();
    let (host, mut app) = hosted();
    drain(&mut app, ElevationVerb::Ready);
    let bystander = f.file("untouched", &["a.txt"]);

    app.request(
        &header(ElevationOperation::Rename),
        &[
            "{not json at all".to_string(),
            f.rename_item("a.txt", "c.txt"),
        ],
    );

    assert!(
        app.receive_all()
            .iter()
            .any(|m| m.verb == ElevationVerb::Fatal)
    );
    assert!(join(&host, PATIENCE));
    assert!(exists(&bystander));
    assert!(!exists(&f.p(&["c.txt"])));
}

// --- doing the work ---

#[test]
fn it_renames_and_reports_what_it_did() {
    let f = Fixture::new();
    let (host, mut app) = hosted();
    drain(&mut app, ElevationVerb::Ready);
    f.file("payload", &["a.txt"]);

    app.request(
        &header(ElevationOperation::Rename),
        &[f.rename_item("a.txt", "b.txt")],
    );
    let said = app.receive_all();
    join(&host, PATIENCE);

    assert_eq!("payload", std::fs::read_to_string(f.p(&["b.txt"])).unwrap());
    assert!(said.iter().any(|m| m.verb == ElevationVerb::Done));
    assert!(said.iter().any(|m| m.verb == ElevationVerb::End));
    assert!(!said.iter().any(|m| m.verb == ElevationVerb::Fault));
}

#[test]
fn it_moves_and_reports_progress_as_it_goes() {
    let f = Fixture::new();
    let (host, mut app) = hosted();
    drain(&mut app, ElevationVerb::Ready);
    let source = f.file("payload", &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);

    app.request(
        &ElevationHost::write(
            &ElevationHeader::new(ElevationOperation::TransferMove).with_destination(&dest),
        ),
        &[ElevationHost::write(&ElevationTransferItem {
            item: PlannedTransfer::new(&source, false, &format!("{dest}\\a.txt"), false),
            resolution: ConflictResolution::KeepBoth,
        })],
    );

    let said = app.receive_all();
    join(&host, PATIENCE);

    assert_eq!(
        "payload",
        std::fs::read_to_string(format!("{dest}\\a.txt")).unwrap()
    );
    assert!(!exists(&source));
    assert!(said.iter().any(|m| m.verb == ElevationVerb::Progress));
    assert!(said.iter().any(|m| m.verb == ElevationVerb::End));
}

#[test]
fn it_creates_the_one_item_a_creation_names() {
    let f = Fixture::new();
    let (host, mut app) = hosted();
    drain(&mut app, ElevationVerb::Ready);

    app.request(
        &header(ElevationOperation::NewItem),
        &[ElevationHost::write(&NewItemPlan {
            directory: f.root.to_string_lossy().into_owned(),
            name: "notes.txt".to_string(),
            kind: NewItemKind::File,
            template_path: None,
            rejected: None,
        })],
    );

    let said = app.receive_all();
    join(&host, PATIENCE);

    assert!(exists(&f.p(&["notes.txt"])));
    assert!(said.iter().any(|m| m.verb == ElevationVerb::Done));
}

#[test]
fn a_failure_is_reported_without_stopping_the_rest() {
    let f = Fixture::new();
    let (host, mut app) = hosted();
    drain(&mut app, ElevationVerb::Ready);
    f.file("here", &["a.txt"]);

    app.request(
        &header(ElevationOperation::Rename),
        &[
            f.rename_item("missing.txt", "gone.txt"),
            f.rename_item("a.txt", "b.txt"),
        ],
    );

    let said = app.receive_all();
    join(&host, PATIENCE);

    assert!(said.iter().any(|m| m.verb == ElevationVerb::Fault));
    assert!(exists(&f.p(&["b.txt"])));
}

// --- the app going away ---

#[test]
fn losing_the_app_ends_the_run() {
    let (host, mut app) = hosted();
    drain(&mut app, ElevationVerb::Ready);

    app.close();

    assert!(
        join(&host, PATIENCE),
        "the helper should end when the pipe does."
    );
}

// --- helpers ---

/// The app end of the elevated-operation pipe, driven by hand. It exists because the only thing
/// standing between a test and the real helper is a UAC prompt, which no test can answer. Port of
/// `ElevationPeer` in PipeFakes.cs.
struct ElevationPeer {
    writer: DuplexEnd,
    reader: LineReader<DuplexEnd>,
}

impl ElevationPeer {
    fn new(stream: DuplexEnd) -> ElevationPeer {
        let reader = stream.clone();
        ElevationPeer {
            writer: stream,
            reader: LineReader::new(reader, 64 * 1024),
        }
    }

    fn send(&mut self, verb: ElevationVerb, payload: &str) {
        let _ = write_line(
            &mut self.writer,
            &ElevationProtocol::format(&ElevationMessage::new(verb, payload)),
        );
    }

    /// Sends a line the protocol will refuse, to prove a session survives one.
    fn send_raw(&mut self, line: &str) {
        let _ = write_line(&mut self.writer, line);
    }

    /// Greets, then sends a whole request in the order the client does.
    fn request(&mut self, header: &str, items: &[String]) {
        self.send(
            ElevationVerb::Hello,
            &ElevationProtocol::PROTOCOL_VERSION.to_string(),
        );
        self.send(ElevationVerb::Begin, header);
        for item in items {
            self.send(ElevationVerb::Item, item);
        }
        self.send(ElevationVerb::Go, "");
    }

    /// The next parseable message, skipping anything malformed. None at end of stream.
    fn receive(&mut self) -> Option<ElevationMessage> {
        loop {
            let line = self.reader.read_line().ok()??;
            if let Some(message) = ElevationProtocol::try_parse(&line) {
                return Some(message);
            }
        }
    }

    /// Everything the helper says until it stops, which is what a whole run looks like from this
    /// end.
    fn receive_all(&mut self) -> Vec<ElevationMessage> {
        let mut messages = Vec::new();
        while let Some(message) = self.receive() {
            messages.push(message);
        }
        messages
    }

    fn close(&self) {
        self.writer.close();
    }
}
