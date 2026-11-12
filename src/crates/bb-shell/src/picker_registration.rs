//! Where this app says it can be asked for a file: one value under the current user's hive naming
//! the executable that last started, which another program (BertCut) starts with `--pick`.
//!
//! Why a value of its own rather than the installer's Uninstall key: a copy built from source, or
//! unpacked somewhere, is installed nowhere and has no such key - and the value's presence is also
//! the capability, since only a build that answers `--pick` writes it, so the asker needs no
//! version to compare. Every ordinary start rewrites it (the last copy run is the one asked), the
//! uninstall hook removes it, and an asker must still check the file is there: a build directory
//! can be deleted without anyone telling the registry.

use crate::registry::{Hive, Registry, RegistryResult};

pub struct PickerRegistration;

impl PickerRegistration {
    /// The contract with the asker: `HKCU\Software\BertBrowser`, value `PickerExe`.
    pub const KEY: &'static str = r"Software\BertBrowser";
    pub const VALUE: &'static str = "PickerExe";

    /// Names `exe` as the copy to ask. Writes nothing when it already is.
    pub fn register(registry: &dyn Registry, exe: &str) -> RegistryResult<()> {
        if exe.is_empty() {
            return Ok(());
        }
        let now = registry.string_value(Hive::CurrentUser, Self::KEY, Self::VALUE)?;
        if now.is_some_and(|n| n.eq_ignore_ascii_case(exe)) {
            return Ok(());
        }
        registry.set_string(Hive::CurrentUser, Self::KEY, Self::VALUE, exe)
    }

    /// Names the running executable. A failure costs the asker its picker, never this app its start.
    pub fn register_this_exe(registry: &dyn Registry) {
        if let Ok(exe) = std::env::current_exe() {
            let _ = Self::register(registry, &exe.to_string_lossy());
        }
    }

    /// Takes the value away, and the key with it when nothing else is in it.
    pub fn unregister(registry: &dyn Registry) -> RegistryResult<()> {
        registry.delete_value(Hive::CurrentUser, Self::KEY, Self::VALUE)?;
        if registry.is_empty(Hive::CurrentUser, Self::KEY)? == Some(true) {
            registry.delete_key(Hive::CurrentUser, Self::KEY)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::MemoryRegistry;

    fn value(r: &MemoryRegistry) -> Option<String> {
        r.string_value(
            Hive::CurrentUser,
            PickerRegistration::KEY,
            PickerRegistration::VALUE,
        )
        .unwrap()
    }

    #[test]
    fn the_last_copy_started_is_the_one_named() {
        let r = MemoryRegistry::new();
        PickerRegistration::register(&r, r"C:\A\BertBrowser.exe").unwrap();
        PickerRegistration::register(&r, r"D:\B\BertBrowser.exe").unwrap();
        assert_eq!(Some(r"D:\B\BertBrowser.exe".to_string()), value(&r));
    }

    #[test]
    fn nothing_is_registered_for_no_path() {
        let r = MemoryRegistry::new();
        PickerRegistration::register(&r, "").unwrap();
        assert_eq!(None, value(&r));
    }

    #[test]
    fn unregistering_leaves_nothing_of_ours_and_everything_of_anyone_elses() {
        let r = MemoryRegistry::new();
        PickerRegistration::register(&r, r"C:\A\BertBrowser.exe").unwrap();
        PickerRegistration::unregister(&r).unwrap();
        let key_exists = |r: &MemoryRegistry| {
            r.key_exists(Hive::CurrentUser, PickerRegistration::KEY)
                .unwrap()
        };
        assert!(!key_exists(&r));

        PickerRegistration::register(&r, r"C:\A\BertBrowser.exe").unwrap();
        r.set_string(Hive::CurrentUser, PickerRegistration::KEY, "Other", "x")
            .unwrap();
        PickerRegistration::unregister(&r).unwrap();
        assert_eq!(None, value(&r));
        assert!(key_exists(&r));
        // And again, with nothing there, is not an error.
        PickerRegistration::unregister(&r).unwrap();
    }
}
