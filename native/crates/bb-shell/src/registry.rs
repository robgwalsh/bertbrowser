//! The registry as the shell integration sees it: string values under two hives, behind a trait so
//! the folder-handler registration and the run-as lookup can be driven against a fake and never
//! touch the user's machine in a test. The real one reads and writes through `windows-sys`.
//! Spec: App/Interop/FolderHandlerRegistry.cs, RunAsVerbRegistry.cs.

use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Hive {
    CurrentUser,
    ClassesRoot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryError(pub String);

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "registry: {}", self.0)
    }
}

impl std::error::Error for RegistryError {}

pub type RegistryResult<T> = Result<T, RegistryError>;

/// String values and keys. A missing key or value is `Ok(None)` / `Ok(())`, never an error; an
/// error is the registry refusing (access, a name too long).
pub trait Registry: Send + Sync {
    /// The value `name` of `key` (the default value when `name` is empty), environment variables in
    /// an expandable string already expanded.
    fn string_value(&self, hive: Hive, key: &str, name: &str) -> RegistryResult<Option<String>>;
    fn key_exists(&self, hive: Hive, key: &str) -> RegistryResult<bool>;
    /// Creates the key (and its parents) if need be.
    fn set_string(&self, hive: Hive, key: &str, name: &str, data: &str) -> RegistryResult<()>;
    /// The key and everything under it.
    fn delete_tree(&self, hive: Hive, key: &str) -> RegistryResult<()>;
    fn delete_value(&self, hive: Hive, key: &str, name: &str) -> RegistryResult<()>;
    /// True when the key has no subkeys and no values; None when there is no such key.
    fn is_empty(&self, hive: Hive, key: &str) -> RegistryResult<Option<bool>>;
    /// The key alone; it must have no subkeys.
    fn delete_key(&self, hive: Hive, key: &str) -> RegistryResult<()>;
}

// ---- the fake -------------------------------------------------------------------------------

/// An in-memory registry: case-insensitive paths and value names, parents created implicitly.
#[derive(Default)]
pub struct MemoryRegistry {
    keys: Mutex<BTreeMap<(Hive, String), BTreeMap<String, String>>>,
}

fn norm(key: &str) -> String {
    key.trim_matches('\\').to_uppercase()
}

impl MemoryRegistry {
    pub fn new() -> MemoryRegistry {
        MemoryRegistry::default()
    }

    /// Every (key, value name, data) stored, for a test to compare before and after.
    pub fn dump(&self) -> Vec<(Hive, String, String, String)> {
        let keys = self.keys.lock().unwrap();
        let mut out = Vec::new();
        for ((hive, key), values) in keys.iter() {
            if values.is_empty() {
                out.push((*hive, key.clone(), String::new(), String::new()));
            }
            for (name, data) in values {
                out.push((*hive, key.clone(), name.clone(), data.clone()));
            }
        }
        out
    }
}

impl Registry for MemoryRegistry {
    fn string_value(&self, hive: Hive, key: &str, name: &str) -> RegistryResult<Option<String>> {
        let keys = self.keys.lock().unwrap();
        Ok(keys
            .get(&(hive, norm(key)))
            .and_then(|v| v.get(&name.to_uppercase()))
            .cloned())
    }

    fn key_exists(&self, hive: Hive, key: &str) -> RegistryResult<bool> {
        Ok(self.keys.lock().unwrap().contains_key(&(hive, norm(key))))
    }

    fn set_string(&self, hive: Hive, key: &str, name: &str, data: &str) -> RegistryResult<()> {
        let mut keys = self.keys.lock().unwrap();
        let path = norm(key);
        let mut prefix = String::new();
        for part in path.split('\\') {
            if !prefix.is_empty() {
                prefix.push('\\');
            }
            prefix.push_str(part);
            keys.entry((hive, prefix.clone())).or_default();
        }
        keys.get_mut(&(hive, path))
            .expect("created above")
            .insert(name.to_uppercase(), data.to_string());
        Ok(())
    }

    fn delete_tree(&self, hive: Hive, key: &str) -> RegistryResult<()> {
        let path = norm(key);
        let below = format!("{path}\\");
        self.keys
            .lock()
            .unwrap()
            .retain(|(h, k), _| !(*h == hive && (*k == path || k.starts_with(&below))));
        Ok(())
    }

    fn delete_value(&self, hive: Hive, key: &str, name: &str) -> RegistryResult<()> {
        if let Some(values) = self.keys.lock().unwrap().get_mut(&(hive, norm(key))) {
            values.remove(&name.to_uppercase());
        }
        Ok(())
    }

    fn is_empty(&self, hive: Hive, key: &str) -> RegistryResult<Option<bool>> {
        let keys = self.keys.lock().unwrap();
        let path = norm(key);
        let Some(values) = keys.get(&(hive, path.clone())) else {
            return Ok(None);
        };
        let below = format!("{path}\\");
        let has_child = keys
            .keys()
            .any(|(h, k)| *h == hive && k.starts_with(&below));
        Ok(Some(values.is_empty() && !has_child))
    }

    fn delete_key(&self, hive: Hive, key: &str) -> RegistryResult<()> {
        let mut keys = self.keys.lock().unwrap();
        let path = norm(key);
        let below = format!("{path}\\");
        if keys
            .keys()
            .any(|(h, k)| *h == hive && k.starts_with(&below))
        {
            return Err(RegistryError("the key has subkeys".to_string()));
        }
        keys.remove(&(hive, path));
        Ok(())
    }
}

// ---- the real one ---------------------------------------------------------------------------

/// The machine's registry. Writes go to `HKEY_CURRENT_USER` only; `HKEY_CLASSES_ROOT` is read.
pub struct SystemRegistry;

mod win32 {
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS,
    };
    use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_EXPAND_SZ,
        REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteKeyW,
        RegDeleteTreeW, RegDeleteValueW, RegOpenKeyExW, RegQueryInfoKeyW, RegQueryValueExW,
        RegSetValueExW,
    };

    use super::{Hive, RegistryError, RegistryResult};

    pub fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn root(hive: Hive) -> HKEY {
        match hive {
            Hive::CurrentUser => HKEY_CURRENT_USER,
            Hive::ClassesRoot => HKEY_CLASSES_ROOT,
        }
    }

    fn fail(what: &str, code: u32) -> RegistryError {
        RegistryError(format!(
            "{what}: {}",
            std::io::Error::from_raw_os_error(code as i32)
        ))
    }

    fn missing(code: u32) -> bool {
        code == ERROR_FILE_NOT_FOUND || code == ERROR_PATH_NOT_FOUND
    }

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: closing a key this struct opened.
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn open(hive: Hive, key: &str, access: u32) -> RegistryResult<Option<Key>> {
        let path = wide(key);
        let mut handle: HKEY = null_mut();
        // SAFETY: `path` is NUL-terminated and outlives the call.
        let code = unsafe { RegOpenKeyExW(root(hive), path.as_ptr(), 0, access, &mut handle) };
        match code {
            ERROR_SUCCESS => Ok(Some(Key(handle))),
            c if missing(c) => Ok(None),
            c => Err(fail("open", c)),
        }
    }

    pub fn string_value(hive: Hive, key: &str, name: &str) -> RegistryResult<Option<String>> {
        let Some(opened) = open(hive, key, KEY_READ)? else {
            return Ok(None);
        };
        let value = wide(name);
        let mut kind = 0u32;
        let mut size = 0u32;
        // SAFETY: a size query; no buffer is passed.
        let code = unsafe {
            RegQueryValueExW(
                opened.0,
                value.as_ptr(),
                null(),
                &mut kind,
                null_mut(),
                &mut size,
            )
        };
        if missing(code) {
            return Ok(None);
        }
        if code != ERROR_SUCCESS && code != ERROR_MORE_DATA {
            return Err(fail("query", code));
        }
        if kind != REG_SZ && kind != REG_EXPAND_SZ {
            return Ok(None);
        }
        let mut buffer = vec![0u16; (size as usize).div_ceil(2) + 1];
        let mut capacity = (buffer.len() * 2) as u32;
        // SAFETY: the buffer holds `capacity` bytes.
        let code = unsafe {
            RegQueryValueExW(
                opened.0,
                value.as_ptr(),
                null(),
                &mut kind,
                buffer.as_mut_ptr().cast(),
                &mut capacity,
            )
        };
        if code != ERROR_SUCCESS {
            return Err(fail("query", code));
        }
        let length = (capacity as usize / 2).min(buffer.len());
        let end = buffer[..length]
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(length);
        let text = String::from_utf16_lossy(&buffer[..end]);
        Ok(Some(if kind == REG_EXPAND_SZ {
            expand(&text)
        } else {
            text
        }))
    }

    fn expand(text: &str) -> String {
        let source = wide(text);
        // SAFETY: a size query with no buffer.
        let needed = unsafe { ExpandEnvironmentStringsW(source.as_ptr(), null_mut(), 0) };
        if needed == 0 {
            return text.to_string();
        }
        let mut out = vec![0u16; needed as usize];
        // SAFETY: the buffer holds `needed` characters.
        let written =
            unsafe { ExpandEnvironmentStringsW(source.as_ptr(), out.as_mut_ptr(), needed) };
        if written == 0 || written > needed {
            return text.to_string();
        }
        String::from_utf16_lossy(&out[..(written as usize).saturating_sub(1)])
    }

    pub fn key_exists(hive: Hive, key: &str) -> RegistryResult<bool> {
        Ok(open(hive, key, KEY_READ)?.is_some())
    }

    pub fn set_string(hive: Hive, key: &str, name: &str, data: &str) -> RegistryResult<()> {
        let path = wide(key);
        let mut handle: HKEY = null_mut();
        // SAFETY: `path` is NUL-terminated; a null class and security descriptor are the defaults.
        let code = unsafe {
            RegCreateKeyExW(
                root(hive),
                path.as_ptr(),
                0,
                null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                null(),
                &mut handle,
                null_mut(),
            )
        };
        if code != ERROR_SUCCESS {
            return Err(fail("create", code));
        }
        let key = Key(handle);
        let value = wide(name);
        let bytes = wide(data);
        // SAFETY: `bytes` is a NUL-terminated UTF-16 string of the length passed.
        let code = unsafe {
            RegSetValueExW(
                key.0,
                value.as_ptr(),
                0,
                REG_SZ,
                bytes.as_ptr().cast(),
                (bytes.len() * 2) as u32,
            )
        };
        if code != ERROR_SUCCESS {
            return Err(fail("set", code));
        }
        Ok(())
    }

    pub fn delete_tree(hive: Hive, key: &str) -> RegistryResult<()> {
        let path = wide(key);
        // SAFETY: `path` is NUL-terminated and outlives the call.
        let code = unsafe { RegDeleteTreeW(root(hive), path.as_ptr()) };
        if code == ERROR_SUCCESS || missing(code) {
            Ok(())
        } else {
            Err(fail("delete tree", code))
        }
    }

    pub fn delete_value(hive: Hive, key: &str, name: &str) -> RegistryResult<()> {
        let Some(opened) = open(hive, key, KEY_SET_VALUE)? else {
            return Ok(());
        };
        let value = wide(name);
        // SAFETY: `value` is NUL-terminated and outlives the call.
        let code = unsafe { RegDeleteValueW(opened.0, value.as_ptr()) };
        if code == ERROR_SUCCESS || missing(code) {
            Ok(())
        } else {
            Err(fail("delete value", code))
        }
    }

    pub fn is_empty(hive: Hive, key: &str) -> RegistryResult<Option<bool>> {
        let Some(opened) = open(hive, key, KEY_READ)? else {
            return Ok(None);
        };
        let mut subkeys = 0u32;
        let mut values = 0u32;
        // SAFETY: only the two counts are asked for; the other outputs are null.
        let code = unsafe {
            RegQueryInfoKeyW(
                opened.0,
                null_mut(),
                null_mut(),
                null(),
                &mut subkeys,
                null_mut(),
                null_mut(),
                &mut values,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        if code != ERROR_SUCCESS {
            return Err(fail("query info", code));
        }
        Ok(Some(subkeys == 0 && values == 0))
    }

    pub fn delete_key(hive: Hive, key: &str) -> RegistryResult<()> {
        let path = wide(key);
        // SAFETY: `path` is NUL-terminated and outlives the call.
        let code = unsafe { RegDeleteKeyW(root(hive), path.as_ptr()) };
        if code == ERROR_SUCCESS || missing(code) {
            Ok(())
        } else {
            Err(fail("delete key", code))
        }
    }
}

impl Registry for SystemRegistry {
    fn string_value(&self, hive: Hive, key: &str, name: &str) -> RegistryResult<Option<String>> {
        win32::string_value(hive, key, name)
    }

    fn key_exists(&self, hive: Hive, key: &str) -> RegistryResult<bool> {
        win32::key_exists(hive, key)
    }

    fn set_string(&self, hive: Hive, key: &str, name: &str, data: &str) -> RegistryResult<()> {
        // The only hive anything here writes to. Writing the machine's classes would need rights the
        // app deliberately does not have, and a mistake there would be everyone's.
        if hive != Hive::CurrentUser {
            return Err(RegistryError(
                "only the current user's hive is written".into(),
            ));
        }
        win32::set_string(hive, key, name, data)
    }

    fn delete_tree(&self, hive: Hive, key: &str) -> RegistryResult<()> {
        if hive != Hive::CurrentUser {
            return Err(RegistryError(
                "only the current user's hive is written".into(),
            ));
        }
        win32::delete_tree(hive, key)
    }

    fn delete_value(&self, hive: Hive, key: &str, name: &str) -> RegistryResult<()> {
        if hive != Hive::CurrentUser {
            return Err(RegistryError(
                "only the current user's hive is written".into(),
            ));
        }
        win32::delete_value(hive, key, name)
    }

    fn is_empty(&self, hive: Hive, key: &str) -> RegistryResult<Option<bool>> {
        win32::is_empty(hive, key)
    }

    fn delete_key(&self, hive: Hive, key: &str) -> RegistryResult<()> {
        if hive != Hive::CurrentUser {
            return Err(RegistryError(
                "only the current user's hive is written".into(),
            ));
        }
        win32::delete_key(hive, key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fake_creates_parents_and_ignores_case() {
        let r = MemoryRegistry::new();
        r.set_string(Hive::CurrentUser, r"Software\A\B", "", "x")
            .unwrap();
        assert!(r.key_exists(Hive::CurrentUser, r"software\a").unwrap());
        assert_eq!(
            Some("x".to_string()),
            r.string_value(Hive::CurrentUser, r"SOFTWARE\A\b", "")
                .unwrap()
        );
        assert_eq!(
            Some(false),
            r.is_empty(Hive::CurrentUser, r"Software\A").unwrap()
        );
        assert_eq!(None, r.is_empty(Hive::CurrentUser, r"Software\Z").unwrap());
    }

    #[test]
    fn delete_tree_takes_the_subkeys_and_delete_key_refuses_them() {
        let r = MemoryRegistry::new();
        r.set_string(Hive::CurrentUser, r"S\A\B", "v", "1").unwrap();
        assert!(r.delete_key(Hive::CurrentUser, r"S\A").is_err());
        r.delete_tree(Hive::CurrentUser, r"S\A").unwrap();
        assert!(!r.key_exists(Hive::CurrentUser, r"S\A\B").unwrap());
        assert!(r.key_exists(Hive::CurrentUser, "S").unwrap());
    }

    #[test]
    fn the_system_registry_refuses_to_write_the_classes_hive() {
        let r = SystemRegistry;
        assert!(
            r.set_string(Hive::ClassesRoot, "BertBrowserTest", "", "x")
                .is_err()
        );
        assert!(r.delete_tree(Hive::ClassesRoot, "BertBrowserTest").is_err());
    }

    #[test]
    fn the_system_registry_reads_a_value_that_exists_everywhere() {
        // Every Windows has a default command for the folder open verb's class.
        let r = SystemRegistry;
        assert!(r.key_exists(Hive::ClassesRoot, ".txt").unwrap());
        assert_eq!(
            None,
            r.string_value(Hive::ClassesRoot, "Zzz.No.Such.Key", "")
                .unwrap()
        );
    }
}
