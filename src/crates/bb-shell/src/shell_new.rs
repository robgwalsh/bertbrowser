//! Reads the `ShellNew` keys Windows builds Explorer's New menu from. Spec: App/Interop/ShellNewRegistry.cs,
//! App/Services/ShellNewCatalog.cs.
//!
//! **Read-only, and that is the whole contract.** Every key is opened for reading and nothing here
//! creates, sets or deletes a value: registering a new-file type with the shell would change
//! Explorer's own New menu machine-wide. BertBrowser keeps its own list and only reads this one to
//! fill it. Nothing but raw values leaves the registry walk; which of them are worth offering, what a
//! type is called and where its template is are `ShellNewImport`'s to decide, in Core.

use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use bb_core::newitem::{NewFileTemplate, ShellNewEntry, ShellNewImport, ShellNewKind};
use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CLASSES_ROOT, KEY_READ, REG_BINARY, REG_EXPAND_SZ, REG_SZ, RegCloseKey,
    RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW,
};
use windows_sys::Win32::UI::Shell::SHLoadIndirectString;

/// Every type Windows offers that is worth a menu entry. Walks all of `HKEY_CLASSES_ROOT`, so it
/// belongs off the UI thread. Never fails: a key that cannot be read is one fewer type.
pub fn read_templates(templates_dir: &Path) -> Vec<NewFileTemplate> {
    ShellNewImport::to_templates(
        &read_entries(),
        load_indirect_string,
        |p| Path::new(p).is_file(),
        &template_roots(),
        |extension, data| save_template_data(templates_dir, extension, data),
    )
}

/// Writes a registry-held template out to a real file, once, so everything downstream sees one
/// shape: a template is either empty or a file on disk.
fn save_template_data(dir: &Path, extension: &str, data: &[u8]) -> Option<String> {
    std::fs::create_dir_all(dir).ok()?;
    let path: PathBuf = dir.join(format!(
        "shellnew-{}",
        extension.trim_start_matches('.').to_lowercase()
    ));
    std::fs::write(&path, data).ok()?;
    Some(path.to_string_lossy().into_owned())
}

/// Where a bare template name is looked for: the user's Templates folder, then Windows' own.
fn template_roots() -> Vec<String> {
    let mut roots = Vec::new();
    if let Some(app_data) = std::env::var_os("APPDATA").filter(|v| !v.is_empty()) {
        roots.push(
            Path::new(&app_data)
                .join("Microsoft")
                .join("Windows")
                .join("Templates")
                .to_string_lossy()
                .into_owned(),
        );
    }
    if let Some(windows) = std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("windir"))
        .filter(|v| !v.is_empty())
    {
        roots.push(
            Path::new(&windows)
                .join("ShellNew")
                .to_string_lossy()
                .into_owned(),
        );
    }
    roots
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Resolves an `@%SystemRoot%\system32\notepad.exe,-469` reference, or None.
fn load_indirect_string(reference: &str) -> Option<String> {
    let source = wide(reference);
    let mut buffer = vec![0u16; 1024];
    // SAFETY: `source` is NUL-terminated; the buffer holds `buffer.len()` characters.
    let hr = unsafe {
        SHLoadIndirectString(
            source.as_ptr(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            null_mut(),
        )
    };
    if hr != 0 {
        return None;
    }
    let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..end]))
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: closing a key this struct opened.
        unsafe { RegCloseKey(self.0) };
    }
}

impl Key {
    fn open(parent: HKEY, name: &str) -> Option<Key> {
        let path = wide(name);
        let mut handle: HKEY = null_mut();
        // SAFETY: `path` is NUL-terminated and outlives the call; read access only.
        let code = unsafe { RegOpenKeyExW(parent, path.as_ptr(), 0, KEY_READ, &mut handle) };
        (code == ERROR_SUCCESS).then_some(Key(handle))
    }

    fn subkeys(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut name = [0u16; 512];
        for index in 0.. {
            let mut length = name.len() as u32;
            // SAFETY: `name` holds `length` characters; the optional out-parameters are null.
            let code = unsafe {
                RegEnumKeyExW(
                    self.0,
                    index,
                    name.as_mut_ptr(),
                    &mut length,
                    null(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                )
            };
            if code == ERROR_MORE_DATA {
                continue; // a name longer than any extension or ProgID worth reading
            }
            if code != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..length as usize]));
        }
        out
    }

    /// The value's type and bytes, or None when there is no such value. `name` empty is the default.
    fn value(&self, name: &str) -> Option<(u32, Vec<u8>)> {
        let value = wide(name);
        let mut kind = 0u32;
        let mut size = 0u32;
        // SAFETY: a size query; no buffer is passed.
        let code = unsafe {
            RegQueryValueExW(
                self.0,
                value.as_ptr(),
                null(),
                &mut kind,
                null_mut(),
                &mut size,
            )
        };
        if code != ERROR_SUCCESS && code != ERROR_MORE_DATA {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        // SAFETY: the buffer holds `size` bytes.
        let code = unsafe {
            RegQueryValueExW(
                self.0,
                value.as_ptr(),
                null(),
                &mut kind,
                data.as_mut_ptr(),
                &mut size,
            )
        };
        if code != ERROR_SUCCESS {
            return None;
        }
        data.truncate(size as usize);
        Some((kind, data))
    }

    fn string(&self, name: &str) -> Option<String> {
        let (kind, data) = self.value(name)?;
        if kind != REG_SZ && kind != REG_EXPAND_SZ {
            return None;
        }
        let units: Vec<u16> = data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .take_while(|c| *c != 0)
            .collect();
        Some(String::from_utf16_lossy(&units))
    }
}

fn read_entries() -> Vec<ShellNewEntry> {
    let Some(classes) = Key::open(HKEY_CLASSES_ROOT, "") else {
        return Vec::new();
    };
    classes
        .subkeys()
        .into_iter()
        .filter(|name| name.len() >= 2 && name.starts_with('.'))
        .filter_map(|extension| read_extension(&classes, &extension))
        .collect()
}

fn read_extension(classes: &Key, extension: &str) -> Option<ShellNewEntry> {
    let extension_key = Key::open(classes.0, extension)?;
    // ".docx\ShellNew" is the common shape, but Office and others hang it off the ProgID subkey
    // instead (".docx\Word.Document.12\ShellNew"), so look one level down too.
    let shell_new = Key::open(extension_key.0, "ShellNew").or_else(|| {
        extension_key
            .subkeys()
            .iter()
            .find_map(|child| Key::open(extension_key.0, &format!("{child}\\ShellNew")))
    })?;
    let mut entry = ShellNewEntry::new(extension, None, ShellNewKind::NullFile);
    entry.label = label_of(classes, &extension_key);
    // Command is reported rather than hidden, so the decision to drop it stays in one place in Core.
    if shell_new.value("Command").is_some() {
        entry.kind = ShellNewKind::Command;
    } else if shell_new.value("FileName").is_some() {
        entry.kind = ShellNewKind::FileName;
        entry.file_name = Some(shell_new.string("FileName").filter(|f| !f.is_empty())?);
    } else if let Some((kind, data)) = shell_new.value("Data") {
        if kind != REG_BINARY || data.is_empty() {
            return None;
        }
        entry.kind = ShellNewKind::Data;
        entry.data = Some(data);
    } else if shell_new.value("NullFile").is_none() {
        return None;
    }
    Some(entry)
}

/// The friendly type name, which may still be an unresolved `@dll,-id` reference: an extension key's
/// default value is its ProgID, and the name worth showing hangs off that.
fn label_of(classes: &Key, extension_key: &Key) -> Option<String> {
    let prog_id = extension_key.string("").filter(|p| !p.is_empty())?;
    let prog_key = Key::open(classes.0, &prog_id)?;
    prog_key
        .string("FriendlyTypeName")
        .or_else(|| prog_key.string(""))
}
