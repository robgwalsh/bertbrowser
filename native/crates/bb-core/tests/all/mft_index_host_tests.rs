// Port of tests/BertBrowser.Core.Tests/MftIndexHostTests.cs: the elevated half. What it will
// accept is the security surface of the whole split, so most of this file is about what it refuses
// to do.
use std::sync::Arc;
use std::thread::{self, JoinHandle, sleep};
use std::time::{Duration, Instant};

use bb_core::cancel::CancelToken;
use bb_core::changes::ChangeLogPolicy;
use bb_core::ipc::{IndexProtocol, IndexVerb};
use bb_core::mft::{IndexSessionEnd, MftIndex, MftIndexHost};
use bb_test_support::pipe_fakes::{ControllableIndexService, DuplexPair, ProtocolPeer};

const PATIENCE: Duration = Duration::from_secs(5);

/// Runs a host on its own thread against a peer the test drives as the app.
fn hosted(index: Arc<ControllableIndexService>) -> (JoinHandle<IndexSessionEnd>, ProtocolPeer) {
    let (thread, app, _) = hosted_session(Arc::new(MftIndexHost::new(index)));
    (thread, app)
}

/// One session on an existing host, so a test can end a session and open another against the
/// same running index, which is what an app closing and reopening looks like from here.
fn hosted_session(
    host: Arc<MftIndexHost>,
) -> (JoinHandle<IndexSessionEnd>, ProtocolPeer, Arc<MftIndexHost>) {
    let (host_end, app_end) = DuplexPair::create();
    let runner = host.clone();
    let thread = thread::Builder::new()
        .name("test index host".to_string())
        .spawn(move || runner.run(host_end, &CancelToken::none()))
        .unwrap();
    (thread, ProtocolPeer::new(app_end), host)
}

fn eventually(condition: impl Fn() -> bool, because: &str) {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        sleep(Duration::from_millis(10));
    }
    panic!("{because}");
}

/// `Thread.Join(timeout)`: true when the run returned in time.
fn joins(thread: JoinHandle<IndexSessionEnd>) -> bool {
    let deadline = Instant::now() + PATIENCE;
    while !thread.is_finished() {
        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(10));
    }
    thread.join().is_ok()
}

#[test]
fn greets_with_its_version_and_announces_it_is_ready() {
    let (thread, app) = hosted(Arc::new(ControllableIndexService::new()));

    let hello = app.receive().unwrap();
    assert_eq!(IndexVerb::Hello, hello.verb);
    assert_eq!(
        Some(IndexProtocol::PROTOCOL_VERSION),
        IndexProtocol::version_of(&hello)
    );
    assert_eq!(IndexVerb::Ready, app.receive().unwrap().verb);

    app.close();
    assert!(joins(thread));
}

#[test]
fn starts_indexing_when_asked() {
    let index = Arc::new(ControllableIndexService::new());
    let (thread, app) = hosted(index.clone());
    app.receive();
    app.receive();

    app.send(IndexVerb::Start, "");

    eventually(|| index.starts() == 1, "Start should start the indexer");
    app.close();
    assert!(joins(thread));
}

#[test]
fn applies_the_recording_policy_it_is_sent() {
    let index = Arc::new(ControllableIndexService::new());
    let (thread, app) = hosted(index.clone());
    app.receive();
    app.receive();
    assert!(!index.change_log().enabled);

    app.send(IndexVerb::Record, "24");
    eventually(
        || index.change_log() == ChangeLogPolicy::from_hours(24).unwrap(),
        "Record 24 should turn recording on for a day",
    );

    app.send(IndexVerb::Record, "0");
    eventually(
        || !index.change_log().enabled,
        "Record 0 should turn recording off",
    );

    app.close();
    assert!(joins(thread));
}

/// The point of the helper outliving the app: a second app attaches to an index that finished
/// building while nothing was connected, and must be told about it. IndexRefreshed already fired
/// for that volume and will never fire again, so only a replay can carry it.
#[test]
fn replays_what_it_already_knows_to_a_late_joining_app() {
    let index = Arc::new(ControllableIndexService::new());
    let host = Arc::new(MftIndexHost::new(index.clone()));

    let (first, app1, _) = hosted_session(host.clone());
    app1.receive();
    app1.receive();
    index.begin_building("C");
    index.finish_building("C", r"C:\");
    app1.close();
    assert!(joins(first));

    // A whole app lifetime later, on a brand new connection.
    let (second, app2, _) = hosted_session(host);

    assert_eq!(IndexVerb::Hello, app2.receive().unwrap().verb);
    let complete = app2.receive().unwrap();
    assert_eq!(IndexVerb::Complete, complete.verb);
    assert_eq!(r"C:\", complete.argument);
    // Ready comes last, and means "you now know everything I know".
    assert_eq!(IndexVerb::Ready, app2.receive().unwrap().verb);

    app2.close();
    assert!(joins(second));
}

/// A drive still building when the second app attaches is announced to it too.
#[test]
fn replays_drives_still_building() {
    let index = Arc::new(ControllableIndexService::new());
    let host = Arc::new(MftIndexHost::new(index.clone()));
    index.begin_building("D");

    let (first, app1, _) = hosted_session(host.clone());
    app1.close();
    assert!(joins(first));

    let (second, app2, _) = hosted_session(host);
    let building = app2.receive_one_of(&[IndexVerb::Building]).unwrap();
    assert_eq!("D", building.argument);

    app2.close();
    assert!(joins(second));
}

/// Losing the pipe ends the session, not the index. A second app must not start a second set of
/// volume threads over the top of the ones already running.
#[test]
fn a_second_session_does_not_start_the_index_again() {
    let index = Arc::new(ControllableIndexService::new());
    let host = Arc::new(MftIndexHost::new(index.clone()));

    let (first, app1, _) = hosted_session(host.clone());
    app1.send(IndexVerb::Start, "");
    eventually(
        || index.starts() == 1,
        "the first app should start indexing",
    );
    app1.close();
    assert!(joins(first));

    let (second, app2, _) = hosted_session(host);
    app2.send(IndexVerb::Start, "");
    app2.send(IndexVerb::Ping, "");
    assert_eq!(
        IndexVerb::Pong,
        app2.receive_one_of(&[IndexVerb::Pong]).unwrap().verb
    );

    assert_eq!(1, index.starts());

    app2.close();
    assert!(joins(second));
}

/// Shutdown and a peer that simply vanished are different answers to the caller: one ends the
/// process, the other only ends the session.
#[test]
fn tells_the_caller_whether_it_was_asked_to_stop_or_just_lost_the_app() {
    let index = Arc::new(ControllableIndexService::new());
    let host = Arc::new(MftIndexHost::new(index));

    let (gone, gone_app, _) = hosted_session(host.clone());
    gone_app.close();
    assert_eq!(IndexSessionEnd::PeerGone, gone.join().unwrap());

    let (stop, stop_app, _) = hosted_session(host);
    stop_app.send(IndexVerb::Shutdown, "");
    assert_eq!(IndexSessionEnd::Stop, stop.join().unwrap());
    stop_app.close();
}

/// A second Start must not run a second set of volume threads.
#[test]
fn starts_at_most_once() {
    let index = Arc::new(ControllableIndexService::new());
    let (thread, app) = hosted(index.clone());

    app.send(IndexVerb::Start, "");
    app.send(IndexVerb::Start, "");
    app.send(IndexVerb::Start, "");
    eventually(|| index.starts() == 1, "the first Start should take");
    app.send(IndexVerb::Ping, "");
    assert_eq!(
        IndexVerb::Pong,
        app.receive_one_of(&[IndexVerb::Pong]).unwrap().verb
    );

    assert_eq!(1, index.starts());
    app.close();
    assert!(joins(thread));
}

#[test]
fn reports_building_and_completion_as_it_happens() {
    let index = Arc::new(ControllableIndexService::new());
    let (thread, app) = hosted(index.clone());
    app.send(IndexVerb::Start, "");
    eventually(|| index.starts() == 1, "precondition: started");

    index.begin_building("C");
    let building = app.receive_one_of(&[IndexVerb::Building]).unwrap();
    assert_eq!("C", building.argument);

    index.finish_building("C", r"C:\");
    assert_eq!(
        r"C:\",
        app.receive_one_of(&[IndexVerb::Complete]).unwrap().argument
    );

    app.close();
    assert!(joins(thread));
}

/// A volume that fails reports Idle without a Complete; that is a real outcome.
#[test]
fn reports_idle_for_a_volume_that_never_completes() {
    let index = Arc::new(ControllableIndexService::new());
    let (thread, app) = hosted(index.clone());
    app.send(IndexVerb::Start, "");
    eventually(|| index.starts() == 1, "precondition: started");

    index.begin_building("C");
    assert_eq!(
        "C",
        app.receive_one_of(&[IndexVerb::Building]).unwrap().argument
    );

    index.abandon_building("C");
    let idle = app
        .receive_one_of(&[IndexVerb::Idle, IndexVerb::Complete])
        .unwrap();
    assert_eq!(IndexVerb::Idle, idle.verb);
    assert_eq!("C", idle.argument);

    app.close();
    assert!(joins(thread));
}

/// Each transition is reported once, not on every status change.
#[test]
fn does_not_repeat_a_building_it_already_reported() {
    let index = Arc::new(ControllableIndexService::new());
    let (thread, app) = hosted(index.clone());
    app.send(IndexVerb::Start, "");
    eventually(|| index.starts() == 1, "precondition: started");

    index.begin_building("C");
    index.begin_building("C");
    index.begin_building("D");
    assert_eq!(
        "C",
        app.receive_one_of(&[IndexVerb::Building]).unwrap().argument
    );
    assert_eq!(
        "D",
        app.receive_one_of(&[IndexVerb::Building]).unwrap().argument
    );

    app.close();
    assert!(joins(thread));
}

#[test]
fn answers_a_ping() {
    let (thread, app) = hosted(Arc::new(ControllableIndexService::new()));

    app.send(IndexVerb::Ping, "");

    assert_eq!(
        IndexVerb::Pong,
        app.receive_one_of(&[IndexVerb::Pong]).unwrap().verb
    );
    app.close();
    assert!(joins(thread));
}

#[test]
fn shutdown_ends_the_run() {
    let (thread, app) = hosted(Arc::new(ControllableIndexService::new()));

    app.send(IndexVerb::Shutdown, "");

    assert!(joins(thread), "Shutdown should return from Run");
}

/// The primary shutdown signal, and the one that works when the app crashes rather than exits:
/// the kernel breaks the pipe and the read returns nothing.
#[test]
fn losing_the_app_ends_the_run() {
    let (thread, app) = hosted(Arc::new(ControllableIndexService::new()));

    app.close();

    assert!(joins(thread), "a broken pipe should return from Run");
}

#[test]
fn ignores_a_malformed_message_and_keeps_running() {
    let index = Arc::new(ControllableIndexService::new());
    let (thread, app) = hosted(index.clone());

    app.send_raw("garbage");
    app.send_raw("IDX\tReindex\tC:\\Windows");
    app.send_raw("IDX\tStart\tC:\\Windows");
    app.send(IndexVerb::Start, "");

    eventually(
        || index.starts() == 1,
        "the session should survive malformed lines",
    );
    app.close();
    assert!(joins(thread));
}

/// The property the whole split rests on: nothing a medium-integrity peer can say to this process
/// names a file, a folder or a program.
#[test]
fn a_verb_carrying_a_path_is_not_even_parsed() {
    for line in [
        "IDX\tStart\tC:\\Windows\\System32",
        "IDX\tShutdown\t\\\\server\\share",
        "IDX\tPing\tC:\\",
        "IDX\tHello\tC:\\",
    ] {
        assert!(IndexProtocol::try_parse(line).is_none(), "{line}");
    }
}

#[test]
fn refuses_an_app_speaking_another_protocol_version() {
    let index = Arc::new(ControllableIndexService::new());
    let (thread, app) = hosted(index.clone());

    app.send(
        IndexVerb::Hello,
        &(IndexProtocol::PROTOCOL_VERSION + 1).to_string(),
    );

    assert_eq!(
        IndexVerb::Fatal,
        app.receive_one_of(&[IndexVerb::Fatal]).unwrap().verb
    );
    assert!(joins(thread), "a version mismatch should end the run");
    assert_eq!(0, index.starts());
}

#[test]
fn accepts_a_matching_hello_and_carries_on() {
    let index = Arc::new(ControllableIndexService::new());
    let (thread, app) = hosted(index.clone());

    app.send(
        IndexVerb::Hello,
        &IndexProtocol::PROTOCOL_VERSION.to_string(),
    );
    app.send(IndexVerb::Start, "");

    eventually(
        || index.starts() == 1,
        "a matching version should not end the run",
    );
    app.close();
    assert!(joins(thread));
}

/// The app cannot see this process's exceptions, so an indexer that cannot run has to say so on
/// the wire; otherwise it looks exactly like one that is merely slow, forever.
#[test]
fn an_indexer_that_cannot_start_says_so_before_dying() {
    let index = Arc::new(ControllableIndexService::start_fails_with(
        "The database is locked by another process.",
    ));
    let (thread, app) = hosted(index);

    app.send(IndexVerb::Start, "");

    let fatal = app.receive_one_of(&[IndexVerb::Fatal]).unwrap();
    assert_eq!("The database is locked by another process.", fatal.argument);
    assert!(
        joins(thread),
        "an indexer that cannot start should end the run"
    );
}

/// A multi-line exception message must not become two messages on the wire.
#[test]
fn a_multi_line_failure_is_flattened_into_one_message() {
    let index = Arc::new(ControllableIndexService::start_fails_with(
        "first line\r\nsecond line\tthird",
    ));
    let (thread, app) = hosted(index);

    app.send(IndexVerb::Start, "");

    let fatal = app.receive_one_of(&[IndexVerb::Fatal]).unwrap();
    assert!(!fatal.argument.contains('\n'));
    assert!(!fatal.argument.contains('\r'));
    assert!(fatal.argument.contains("first line"));
    assert!(joins(thread));
}
