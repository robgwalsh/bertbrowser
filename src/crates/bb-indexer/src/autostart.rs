//! Registers and removes the sign-in task, from inside the already-elevated helper. Spec:
//! src/BertBrowser.Indexer/AutoStart.cs.
//!
//! Reached only through `--register-autostart` / `--unregister-autostart`, which the app runs
//! through the same `runas` path it uses to start the helper. One elevation prompt per toggle, and
//! that prompt is the point: it is the user's gesture in front of installing something that runs
//! with an administrator token at every sign-in. A verb on the pipe instead would have let anything
//! reaching that pipe install the same persistence with no gesture at all.
//!
//! The Task Scheduler COM API rather than `schtasks.exe`: `/Create /XML` needs the definition
//! written to a file first, from an elevated process, with a window between writing it and the
//! tool reading it. `ITaskFolder::RegisterTask` takes the XML as a string. The typed interfaces of
//! the `windows` crate stand in for the C#'s late binding: hand-declared vtables would make a wrong
//! slot a call through the wrong function pointer rather than a compile error.
//!
//! **The task runs an administrator-only copy**, `<Program Files>\BertBrowser\bb-indexer.exe`, put
//! there by `register` (`InstalledHelper::install`), never the file beside the app, which a
//! medium-integrity process can replace. **Untestable here**: registering needs elevation and
//! changes the machine. The definition it registers is `IndexerAutoStartTask::build_xml` and the
//! copy is `InstalledHelper`, which is what the tests pin.

use bb_index::ipc::{IndexEndpoint, IndexerArguments};
use bb_index::mft::{AppDirectoryFault, IndexerAutoStartTask, InstalledHelper};
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::System::TaskScheduler::{
    ITaskFolder, ITaskService, TASK_CREATE_OR_UPDATE, TASK_LOGON_INTERACTIVE_TOKEN, TaskScheduler,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::BSTR;

/// Exit codes of the installed-copy steps. 8, 9 and 10 are taken below and in `main`.
const EXIT_NO_PROGRAM_FILES: i32 = 11;
const EXIT_INSTALL_FAILED: i32 = 12;
const EXIT_SELF_IS_INSTALLED_COPY: i32 = 14;

pub fn register(options: &IndexerArguments) -> i32 {
    let Ok(own_exe) = std::env::current_exe() else {
        eprintln!("Cannot determine this executable's path.");
        return 8;
    };
    let Some(program_files) = InstalledHelper::program_files_directory() else {
        eprintln!("Windows does not say where Program Files is.");
        return EXIT_NO_PROGRAM_FILES;
    };
    let folder = InstalledHelper::folder_under(&program_files);

    // The app this registration is for: where this helper runs from, unless it was told. The
    // installed copy itself cannot answer that.
    let app_dir = match (&options.app_directory, own_exe.parent()) {
        (Some(dir), _) => std::path::PathBuf::from(dir),
        (None, Some(own)) if InstalledHelper::same_directory(Some(own), Some(&folder)) => {
            eprintln!("This is the installed copy; it needs --app-dir to register itself.");
            return EXIT_SELF_IS_INSTALLED_COPY;
        }
        (None, Some(own)) => own.to_path_buf(),
        (None, None) => {
            eprintln!("Cannot determine this executable's folder.");
            return 8;
        }
    };
    // Registering from, or for, a folder that is not this very build would install the wrong helper.
    match InstalledHelper::check_app_directory(&own_exe, &app_dir) {
        Ok(()) => {}
        Err(AppDirectoryFault::AppMissing) => {
            eprintln!("BertBrowser is not installed at the given app directory.");
            return 16;
        }
        Err(AppDirectoryFault::HelperDiffers) => {
            eprintln!("The app directory holds a different build of this helper.");
            return 15;
        }
    }

    let installed = match InstalledHelper::install(&own_exe, &folder) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("{error}");
            return EXIT_INSTALL_FAILED;
        }
    };
    let sid = match IndexEndpoint::current_user_sid() {
        Ok(sid) => sid,
        Err(error) => {
            eprintln!("{error}");
            return 8;
        }
    };
    let xml = IndexerAutoStartTask::build_xml(
        &installed.to_string_lossy(),
        &options.data_directory,
        &app_dir.to_string_lossy(),
        &sid,
    );

    with_root_folder(|folder| unsafe {
        folder
            .RegisterTask(
                &BSTR::from(IndexerAutoStartTask::TASK_NAME),
                &BSTR::from(xml.as_str()),
                TASK_CREATE_OR_UPDATE.0,
                &VARIANT::default(),
                &VARIANT::default(),
                TASK_LOGON_INTERACTIVE_TOKEN,
                &VARIANT::default(),
            )
            .map(|_| ())
    })
}

pub fn unregister() -> i32 {
    let code = with_root_folder(|folder| {
        match unsafe { folder.DeleteTask(&BSTR::from(IndexerAutoStartTask::TASK_NAME), 0) } {
            Ok(()) => Ok(()),
            // Already gone is the outcome that was asked for.
            Err(e) if e.code() == ERROR_FILE_NOT_FOUND.to_hresult() => Ok(()),
            Err(e) => Err(e),
        }
    });
    // Only once the task is gone: a task left pointing at a deleted file would just fail quietly,
    // but a copy left behind with no task is harmless clutter either way. Best effort.
    if code == 0
        && let Some(program_files) = InstalledHelper::program_files_directory()
    {
        InstalledHelper::remove(&InstalledHelper::folder_under(&program_files));
    }
    code
}

fn with_root_folder(work: impl FnOnce(&ITaskFolder) -> windows::core::Result<()>) -> i32 {
    // SAFETY: plain COM initialisation on this thread, balanced by the CoUninitialize below.
    let initialised = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if initialised.is_err() {
        eprintln!("The Task Scheduler service is not available.");
        return 9;
    }
    let code = {
        let outcome: windows::core::Result<()> = (|| {
            let service: ITaskService =
                unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER) }?;
            unsafe {
                service.Connect(
                    &VARIANT::default(),
                    &VARIANT::default(),
                    &VARIANT::default(),
                    &VARIANT::default(),
                )
            }?;
            let folder = unsafe { service.GetFolder(&BSTR::from("\\")) }?;
            work(&folder)
        })();
        match outcome {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("The sign-in task could not be changed: {}", error.message());
                10
            }
        }
    };
    unsafe { CoUninitialize() };
    code
}
