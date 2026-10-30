// Port of tests/BertBrowser.Core.Tests/AccessDeniedTests.cs. The one bit that separates a failure an
// administrator token could fix from every other kind. The rule tests are the rule; the file tests
// are the wiring from Win32, and the negative cases are deliberately the two failures that look most
// like a permission problem from a distance and are neither: a sharing violation and a read-only
// attribute.
//
// The Deny ACE tests must run under a shell that honours ACLs (PowerShell, not the Bash sandbox).
use std::fs::OpenOptions;
use std::io::{self, ErrorKind};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use bb_core::cancel::CancelToken;
use bb_core::delete::{
    DeleteExecutor, DeleteMode, DeleteOutcome, DeletePlanner, DeleteSource, FileSystemDeleteProbe,
};
use bb_core::fs::AccessDenied;
use bb_core::transfer::{
    FileCopier, FileSystemTransferProbe, ProgressFn, TransferError, TransferExecutor,
    TransferOutcome, TransferPlanner, TransferVerb,
};
use bb_test_support::unique_temp_path;

const ERROR_NOT_SAME_DEVICE: i32 = 0x11;
const ERROR_SHARING_VIOLATION: i32 = 0x20;
const ERROR_DISK_FULL: i32 = 0x70;

struct Fixture {
    root: PathBuf,
    denied: Vec<PathBuf>,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-denied", "");
        std::fs::create_dir_all(&root).unwrap();
        Fixture {
            root,
            denied: Vec::new(),
        }
    }

    fn path(&self, parts: &[&str]) -> PathBuf {
        let mut p = self.root.clone();
        for part in parts {
            p.push(part);
        }
        p
    }

    fn file(&self, content: &str, parts: &[&str]) -> PathBuf {
        let p = self.path(parts);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        p
    }

    /// A file Windows will refuse to delete. The Deny ACE goes on the *folder*, since a parent
    /// granting FILE_DELETE_CHILD lets a file be deleted whatever its own DACL says, and the folder is
    /// denied *before* the file is created so the file inherits the denial: adding an inheritable
    /// Deny to a folder that already holds the file leaves the file's own Full Control in place.
    /// Delete rights only; Write is left alone so the file can still be created underneath.
    fn denied_file(&mut self, content: &str, parts: &[&str]) -> PathBuf {
        let p = self.path(parts);
        let folder = p.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&folder).unwrap();
        let user = std::env::var("USERNAME").expect("USERNAME");
        let status = Command::new("icacls")
            .arg(&folder)
            .arg("/deny")
            .arg(format!("{user}:(OI)(CI)(D,DC)"))
            .output()
            .expect("icacls");
        assert!(status.status.success(), "icacls /deny failed");
        self.denied.push(folder);
        std::fs::write(&p, content).unwrap();
        p
    }

    /// A permanent delete of one file through the real planner and executor, holding nothing
    /// anywhere but under the fixture.
    fn delete(&self, path: &Path) -> DeleteOutcome {
        let path = path.to_string_lossy();
        let plan = DeletePlanner::new(&FileSystemDeleteProbe)
            .protecting(&[])
            .plan(&[DeleteSource::new(&path, false)], DeleteMode::Permanent);
        assert!(plan.has_work(), "the planner refused the fixture: {plan:?}");
        DeleteExecutor::new(&FileSystemDeleteProbe)
            .protecting(&[])
            .staging_root(&self.root.to_string_lossy())
            .execute(&plan, &CancelToken::none(), None)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // The Deny ACEs come off first, or the fixture cannot be removed and every later run leaves
        // another undeletable one behind in %TEMP%. /reset replaces the folder's and its children's
        // DACLs with the inherited defaults, which is what makes the fixture removable again.
        for folder in &self.denied {
            let _ = Command::new("icacls")
                .arg(folder)
                .args(["/reset", "/T", "/C", "/Q"])
                .output();
        }
        clear_read_only(&self.root);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn clear_read_only(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            clear_read_only(&path);
        } else if let Ok(meta) = std::fs::metadata(&path) {
            let mut perms = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(&path, perms);
        }
    }
}

// --- the rule ---

#[test]
fn unauthorized_access_is_a_permission_problem() {
    // What the file APIs raise for ERROR_ACCESS_DENIED, and what a copier manufactures for error 5.
    assert!(AccessDenied::caused(Some(&io::Error::new(
        ErrorKind::PermissionDenied,
        "denied"
    ))));
}

#[test]
fn an_io_exception_carrying_the_access_denied_h_result_is_one_too() {
    assert_eq!(AccessDenied::HRESULT, 0x8007_0005_u32 as i32);
    assert!(AccessDenied::caused(Some(&io::Error::from_raw_os_error(
        AccessDenied::WIN32_CODE
    ))));
}

#[test]
fn the_not_same_device_h_result_is_not() {
    assert!(!AccessDenied::caused(Some(&io::Error::from_raw_os_error(
        ERROR_NOT_SAME_DEVICE
    ))));
}

#[test]
fn a_sharing_violation_is_not() {
    assert!(!AccessDenied::caused(Some(&io::Error::from_raw_os_error(
        ERROR_SHARING_VIOLATION
    ))));
}

#[test]
fn other_failures_are_not() {
    let kinds = [
        ("FileNotFound", io::Error::from_raw_os_error(2)),
        ("DirectoryNotFound", io::Error::from_raw_os_error(3)),
        (
            "NotSupported",
            io::Error::new(ErrorKind::Unsupported, "not supported"),
        ),
        (
            "Argument",
            io::Error::new(ErrorKind::InvalidInput, "argument"),
        ),
    ];
    for (name, err) in kinds {
        assert!(!AccessDenied::caused(Some(&err)), "{name}");
    }
}

#[test]
fn a_security_exception_is_not() {
    // Excluded on purpose: a failure that merely *says* "denied", carrying neither the Win32 code nor
    // the permission kind, is speculation. Every false positive here is a UAC prompt that cannot
    // help, so the message is never consulted.
    assert!(!AccessDenied::caused(Some(&io::Error::other(
        "security: access is denied"
    ))));
}

#[test]
fn nothing_is_not() {
    assert!(!AccessDenied::caused(None));
}

// --- the wiring, from Win32 up ---
// The two transfer tests hand `TransferExecutor` a copier that fails the way Windows does and read
// `FailedTransfer.access_denied` off the outcome; the three delete tests go through the real
// `DeleteExecutor`.

/// A copier that fails every file with one error, so the executor's catch clause sees exactly the
/// failure a test names.
struct FailingCopier(fn() -> io::Error);

impl FileCopier for FailingCopier {
    fn copy(
        &self,
        _source: &str,
        _destination: &str,
        _progress: Option<ProgressFn<'_>>,
        _cancel: &CancelToken,
    ) -> Result<(), TransferError> {
        Err(TransferError::Io((self.0)()))
    }

    fn move_file(
        &self,
        source: &str,
        destination: &str,
        progress: Option<ProgressFn<'_>>,
        cancel: &CancelToken,
    ) -> Result<(), TransferError> {
        self.copy(source, destination, progress, cancel)
    }
}

impl Fixture {
    /// A copy of one file into a folder, through the real planner and an executor whose copier fails.
    fn copy_failing(&self, copier: FailingCopier) -> TransferOutcome {
        let source = self.file("payload", &["src", "a.txt"]);
        let destination = self.path(&["dest"]);
        std::fs::create_dir_all(&destination).unwrap();
        let plan = TransferPlanner::default().plan(
            &[source.to_string_lossy().into_owned()],
            &destination.to_string_lossy(),
            TransferVerb::Copy,
        );
        TransferExecutor::new(Box::new(FileSystemTransferProbe), Box::new(copier)).execute(
            &plan,
            None,
            &CancelToken::none(),
            None,
            None,
        )
    }
}

#[test]
fn a_transfer_windows_refuses_is_marked_as_such() {
    // A copier that reports the one failure an administrator token could fix.
    let outcome = Fixture::new().copy_failing(FailingCopier(|| {
        io::Error::new(
            ErrorKind::PermissionDenied,
            "Access to the path 'dest' is denied.",
        )
    }));

    assert_eq!(outcome.failed.len(), 1, "{outcome:?}");
    assert!(outcome.failed[0].access_denied);
}

#[test]
fn a_transfer_that_failed_for_some_other_reason_is_not() {
    // A copier that fails for a reason no token would fix: the case that must never offer elevation.
    let outcome = Fixture::new().copy_failing(FailingCopier(|| {
        io::Error::from_raw_os_error(ERROR_DISK_FULL)
    }));

    assert_eq!(outcome.failed.len(), 1, "{outcome:?}");
    assert!(!outcome.failed[0].access_denied);
}

#[test]
fn a_delete_windows_refuses_is_marked_as_such() {
    let mut f = Fixture::new();
    let victim = f.denied_file("precious", &["locked", "a.txt"]);

    let outcome = f.delete(&victim);

    assert_eq!(outcome.failed.len(), 1, "{outcome:?}");
    let failure = &outcome.failed[0];
    assert!(
        failure.access_denied,
        "expected a permission failure, got '{}'.",
        failure.message
    );
}

#[test]
fn a_file_something_else_has_open_is_not_a_permission_problem() {
    let f = Fixture::new();
    let victim = f.file("busy", &["open", "a.txt"]);
    let _hold = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&victim)
        .unwrap();

    let outcome = f.delete(&victim);

    assert_eq!(outcome.failed.len(), 1, "a file held open was deleted");
    let failure = &outcome.failed[0];
    assert!(
        !failure.access_denied,
        "a sharing violation must not ask for a token: '{}'.",
        failure.message
    );
}

#[test]
fn a_read_only_file_is_not_a_permission_problem_either() {
    // `DeleteExecutor::erase` answers a refusal by clearing the attribute and retrying, so this
    // never reaches the flag at all: it deletes. That order is the reason read-only files do not
    // raise a UAC prompt, and this is what holds it in place.
    let f = Fixture::new();
    let victim = f.file("stubborn", &["ro", "a.txt"]);
    let mut perms = std::fs::metadata(&victim).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&victim, perms).unwrap();

    let outcome = f.delete(&victim);

    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    assert!(!victim.exists());
}
