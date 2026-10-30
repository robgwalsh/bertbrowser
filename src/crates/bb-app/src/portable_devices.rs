//! Phones, cameras and media players under "This PC": folders of the shell namespace that are not
//! the file system, so the list cannot browse them and a click opens Explorer instead. Spec:
//! App/Interop/PortableDevices.cs.

use std::sync::mpsc;
use std::time::Duration;

use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::System::SystemServices::{SFGAO_FILESYSTEM, SFGAO_FOLDER};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_ComputerFolder, IEnumShellItems, IShellItem, KF_FLAG_DEFAULT,
    SHGetKnownFolderItem, SIGDN, SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_NORMALDISPLAY,
};

use crate::seams::{PortableDevice, PortableDeviceSource};

/// How long a waking device may hold the list up before the sidebar goes on without it.
const DEADLINE: Duration = Duration::from_secs(5);

pub(crate) struct ShellPortableDevices;

impl PortableDeviceSource for ShellPortableDevices {
    fn list(&self) -> Vec<PortableDevice> {
        // An apartment of its own, as the shell's folder objects want, and a deadline: a stuck device
        // must never hang anything. A thread that overruns is left to finish on its own.
        let (tx, rx) = mpsc::channel();
        let started = std::thread::Builder::new()
            .name("portable devices".into())
            .spawn(move || {
                let _ = tx.send(enumerate());
            });
        if started.is_err() {
            return Vec::new();
        }
        rx.recv_timeout(DEADLINE).unwrap_or_default()
    }
}

fn enumerate() -> Vec<PortableDevice> {
    // SAFETY: COM on a thread that initialised its own apartment for the duration; every interface is
    // created and dropped inside this scope, and each string the shell hands out is freed once read.
    unsafe {
        let initialised = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let found = (|| -> windows::core::Result<Vec<PortableDevice>> {
            let this_pc: IShellItem =
                SHGetKnownFolderItem(&FOLDERID_ComputerFolder, KF_FLAG_DEFAULT, None)?;
            let items: IEnumShellItems = this_pc.BindToHandler(None, &BHID_EnumItems)?;
            let mut out = Vec::new();
            loop {
                let mut got = [None];
                let mut fetched = 0u32;
                if items.Next(&mut got, Some(&mut fetched)).is_err() || fetched == 0 {
                    break;
                }
                let Some(item) = got[0].take() else { break };
                let Ok(attrs) = item.GetAttributes(SFGAO_FOLDER | SFGAO_FILESYSTEM) else {
                    continue;
                };
                // Drives and the known folders are the file system; a device is a folder that is not.
                if (attrs & SFGAO_FOLDER).0 == 0 || (attrs & SFGAO_FILESYSTEM).0 != 0 {
                    continue;
                }
                let name = display_name(&item, SIGDN_NORMALDISPLAY);
                if name.is_empty() {
                    continue;
                }
                out.push(PortableDevice {
                    name,
                    shell_path: display_name(&item, SIGDN_DESKTOPABSOLUTEPARSING),
                });
            }
            Ok(out)
        })()
        .unwrap_or_default();
        if initialised {
            CoUninitialize();
        }
        found
    }
}

unsafe fn display_name(item: &IShellItem, how: SIGDN) -> String {
    // SAFETY: the PWSTR is the shell's allocation, read once and freed with CoTaskMemFree.
    unsafe {
        match item.GetDisplayName(how) {
            Ok(p) => {
                let text = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const _));
                text
            }
            Err(_) => String::new(),
        }
    }
}
