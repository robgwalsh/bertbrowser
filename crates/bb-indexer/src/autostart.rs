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
//! **Untestable here**: registering needs elevation and changes the machine. The definition it
//! registers is `IndexerAutoStartTask::build_xml`, which is what the tests pin.

use bb_core::ipc::IndexEndpoint;
use bb_core::mft::IndexerAutoStartTask;
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

pub fn register(data_directory: &str) -> i32 {
    let Some(helper_path) = std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
    else {
        eprintln!("Cannot determine this executable's path.");
        return 8;
    };
    let sid = match IndexEndpoint::current_user_sid() {
        Ok(sid) => sid,
        Err(error) => {
            eprintln!("{error}");
            return 8;
        }
    };
    let xml = IndexerAutoStartTask::build_xml(&helper_path, data_directory, &sid);

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
    with_root_folder(|folder| {
        match unsafe { folder.DeleteTask(&BSTR::from(IndexerAutoStartTask::TASK_NAME), 0) } {
            Ok(()) => Ok(()),
            // Already gone is the outcome that was asked for.
            Err(e) if e.code() == ERROR_FILE_NOT_FOUND.to_hresult() => Ok(()),
            Err(e) => Err(e),
        }
    })
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
