//! The one writer of `.lnk` files: the shell's own `IShellLink`, so a link this app writes is a
//! link Windows reads back. Spec: App/Interop/ShellLink.cs.

use std::io;

use bb_core::shortcuts::ShortcutWriter;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize, IPersistFile,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::core::{HSTRING, Interface};

pub(crate) struct ShellLinkWriter;

impl ShortcutWriter for ShellLinkWriter {
    fn write(&self, link_path: &str, target_path: &str) -> io::Result<()> {
        // SAFETY: plain COM calls on interfaces created and dropped inside this scope, on a thread
        // that initialised its own apartment for the duration.
        unsafe {
            let initialised = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
            let result = (|| -> windows::core::Result<()> {
                let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                link.SetPath(&HSTRING::from(target_path))?;
                if let Some(dir) = bb_core::rename::paths::directory_name(target_path) {
                    let _ = link.SetWorkingDirectory(&HSTRING::from(dir));
                }
                let file: IPersistFile = link.cast()?;
                file.Save(&HSTRING::from(link_path), true)?;
                Ok(())
            })();
            if initialised {
                CoUninitialize();
            }
            result.map_err(|e| io::Error::from_raw_os_error(e.code().0))
        }
    }
}
