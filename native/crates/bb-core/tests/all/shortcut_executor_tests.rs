// Port of tests/BertBrowser.Core.Tests/ShortcutExecutorTests.cs. Writing the links a
// `ShortcutPlan` asked for. The writer is faked because what is worth testing here is not the .lnk
// format — that is the shell's job — but the two rules every executor in this app obeys: re-check
// live disk before writing, and never let one item's failure cost the others theirs.
use std::cell::RefCell;
use std::collections::HashMap;
use std::io;

use bb_core::shortcuts::{ShortcutCreation, ShortcutExecutor, ShortcutPlan, ShortcutWriter};
use bb_paths::PathKey;

use crate::shortcut_planner_tests::FakeShortcutProbe;

#[derive(Default)]
struct FakeShortcutWriter {
    failures: HashMap<String, (io::ErrorKind, Option<i32>, String)>,
    written: RefCell<HashMap<String, String>>,
}

impl FakeShortcutWriter {
    fn fail_on(&mut self, link_path: &str, kind: io::ErrorKind, code: Option<i32>, message: &str) {
        self.failures.insert(
            PathKey::canonicalize(link_path).unwrap(),
            (kind, code, message.to_string()),
        );
    }

    fn wrote(&self, link_path: &str) -> bool {
        self.written
            .borrow()
            .contains_key(&PathKey::canonicalize(link_path).unwrap())
    }

    fn target_of(&self, link_path: &str) -> String {
        self.written.borrow()[&PathKey::canonicalize(link_path).unwrap()].clone()
    }

    fn count(&self) -> usize {
        self.written.borrow().len()
    }
}

impl ShortcutWriter for FakeShortcutWriter {
    fn write(&self, link_path: &str, target_path: &str) -> io::Result<()> {
        let key = PathKey::canonicalize(link_path).unwrap();
        if let Some((kind, code, message)) = self.failures.get(&key) {
            return Err(match code {
                Some(code) => io::Error::from_raw_os_error(*code),
                None => io::Error::new(*kind, message.clone()),
            });
        }
        self.written
            .borrow_mut()
            .insert(key, target_path.to_string());
        Ok(())
    }
}

fn probe() -> FakeShortcutProbe {
    let mut probe = FakeShortcutProbe::default();
    probe.add_directory(r"C:\dest");
    probe
}

fn plan_of(creations: &[(&str, &str)]) -> ShortcutPlan {
    ShortcutPlan::new(
        r"C:\dest",
        creations
            .iter()
            .map(|(target, link)| ShortcutCreation::new(target, link))
            .collect(),
        Vec::new(),
    )
}

#[test]
fn writes_one_link_per_creation() {
    let probe = probe();
    let writer = FakeShortcutWriter::default();

    let outcome = ShortcutExecutor::new(&writer, &probe).execute(&plan_of(&[
        (r"C:\src\a.txt", r"C:\dest\a.txt - Shortcut.lnk"),
        (r"C:\src\b.txt", r"C:\dest\b.txt - Shortcut.lnk"),
    ]));

    assert!(outcome.failed.is_empty());
    assert_eq!(2, outcome.created.len());
    assert_eq!(
        r"C:\src\a.txt",
        writer.target_of(r"C:\dest\a.txt - Shortcut.lnk")
    );
    assert_eq!(
        r"C:\src\b.txt",
        writer.target_of(r"C:\dest\b.txt - Shortcut.lnk")
    );
}

/// The plan was made while the menu was open; anything could have appeared since. Writing over it
/// would destroy a file the user never named — so the "(2)" rule runs again against live disk,
/// exactly as the transfer and new-item executors re-check theirs.
#[test]
fn a_name_taken_since_planning_steps_aside_rather_than_overwriting() {
    let mut probe = probe();
    probe.add_file(r"C:\dest\a.txt - Shortcut.lnk");
    let writer = FakeShortcutWriter::default();

    let outcome = ShortcutExecutor::new(&writer, &probe).execute(&plan_of(&[(
        r"C:\src\a.txt",
        r"C:\dest\a.txt - Shortcut.lnk",
    )]));

    assert_eq!(
        vec![r"C:\dest\a.txt - Shortcut (2).lnk".to_string()],
        outcome.created
    );
    assert!(!writer.wrote(r"C:\dest\a.txt - Shortcut.lnk"));
}

#[test]
fn one_failure_does_not_stop_the_others() {
    let probe = probe();
    let mut writer = FakeShortcutWriter::default();
    writer.fail_on(
        r"C:\dest\a.txt - Shortcut.lnk",
        io::ErrorKind::Other,
        None,
        "the disk said no",
    );

    let outcome = ShortcutExecutor::new(&writer, &probe).execute(&plan_of(&[
        (r"C:\src\a.txt", r"C:\dest\a.txt - Shortcut.lnk"),
        (r"C:\src\b.txt", r"C:\dest\b.txt - Shortcut.lnk"),
    ]));

    assert_eq!(
        vec![r"C:\dest\b.txt - Shortcut.lnk".to_string()],
        outcome.created
    );
    assert_eq!(1, outcome.failed.len());
    assert_eq!(r"C:\dest\a.txt - Shortcut.lnk", outcome.failed[0].link_path);
}

/// The one failure an administrator token could fix, flagged so the message can say so rather
/// than blaming the path.
#[test]
fn a_refused_folder_is_reported_as_access_denied() {
    let probe = probe();
    let mut writer = FakeShortcutWriter::default();
    writer.fail_on(
        r"C:\dest\a.txt - Shortcut.lnk",
        io::ErrorKind::PermissionDenied,
        None,
        "no",
    );

    let outcome = ShortcutExecutor::new(&writer, &probe).execute(&plan_of(&[(
        r"C:\src\a.txt",
        r"C:\dest\a.txt - Shortcut.lnk",
    )]));

    assert_eq!(1, outcome.failed.len());
    assert!(outcome.failed[0].access_denied);
}

#[test]
fn an_empty_plan_writes_nothing() {
    let probe = probe();
    let writer = FakeShortcutWriter::default();

    let outcome = ShortcutExecutor::new(&writer, &probe).execute(&ShortcutPlan::empty());

    assert!(outcome.created.is_empty());
    assert!(outcome.failed.is_empty());
    assert_eq!(0, writer.count());
}
