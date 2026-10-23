//! Where the app's data lives: `~\.bertbrowser`. It must **not** live in `%LOCALAPPDATA%\BertBrowser`,
//! because the installer puts the app there and uninstall deletes that directory.
//! Spec: App/Services/AppPaths.cs.

use std::path::{Path, PathBuf};

use bb_paths::PathKey;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{FOLDERID_Profile, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

use super::legacy::LegacyData;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppPathsError {
    /// Windows would not say where the user's profile is and no override is set.
    NoUserProfile,
}

impl std::fmt::Display for AppPathsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppPathsError::NoUserProfile => write!(f, "the user profile folder could not be found"),
        }
    }
}

impl std::error::Error for AppPathsError {}

/// The data directory and the well-known files and folders inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPaths {
    data_dir: PathBuf,
}

impl AppPaths {
    /// Environment variable that moves the whole data directory somewhere else. Read once, before
    /// anything opens the database, so a harness run cannot inherit the user's index, settings or
    /// themes — nor write into them. Nothing in the shipped app sets it.
    pub const OVERRIDE_VARIABLE: &'static str = "BERTBROWSER_DATA_DIR";

    /// The folder under the profile.
    pub const DIRECTORY_NAME: &'static str = ".bertbrowser";

    pub const DB_FILE_NAME: &'static str = "bertbrowser.db";
    pub const SETTINGS_FILE_NAME: &'static str = "settings.json";
    pub const THEMES_DIR_NAME: &'static str = "themes";
    pub const TEMPLATES_DIR_NAME: &'static str = "templates";

    pub fn new(data_dir: impl Into<PathBuf>) -> AppPaths {
        AppPaths {
            data_dir: data_dir.into(),
        }
    }

    /// The data directory for an override value (the variable's text, None or empty when unset)
    /// and the user's profile folder. Pure: the two inputs are what the process would read.
    pub fn resolve(override_value: Option<&str>, user_profile: &Path) -> AppPaths {
        match override_value {
            Some(overridden) if !overridden.is_empty() => AppPaths::new(
                PathKey::normalize_display(overridden)
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| PathBuf::from(overridden)),
            ),
            _ => AppPaths::new(user_profile.join(Self::DIRECTORY_NAME)),
        }
    }

    /// [`resolve`](Self::resolve) over the process environment and the Windows profile folder.
    pub fn from_environment() -> Result<AppPaths, AppPathsError> {
        let overridden = std::env::var(Self::OVERRIDE_VARIABLE).ok();
        if let Some(o) = overridden.as_deref().filter(|o| !o.is_empty()) {
            return Ok(Self::resolve(Some(o), Path::new("")));
        }
        let profile = known_profile_folder()
            .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or(AppPathsError::NoUserProfile)?;
        Ok(Self::resolve(None, &profile))
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join(Self::DB_FILE_NAME)
    }

    pub fn settings_path(&self) -> PathBuf {
        self.data_dir.join(Self::SETTINGS_FILE_NAME)
    }

    /// Where the user's own `*.json` themes live. Created on demand.
    pub fn themes_dir(&self) -> PathBuf {
        self.data_dir.join(Self::THEMES_DIR_NAME)
    }

    /// Where template files for the "New" menu live — both the ones an import writes out of the
    /// registry and any the user points at themselves. Created on demand. Under the data
    /// directory so the override redirects it with everything else.
    pub fn templates_dir(&self) -> PathBuf {
        self.data_dir.join(Self::TEMPLATES_DIR_NAME)
    }

    /// The file whose presence says the native app has already taken this directory over (see
    /// `LegacyData`).
    pub fn native_marker_path(&self) -> PathBuf {
        self.data_dir.join(LegacyData::MARKER_FILE_NAME)
    }
}

/// `SHGetKnownFolderPath(FOLDERID_Profile)`, as `Environment.GetFolderPath(UserProfile)` asked.
fn known_profile_folder() -> Option<PathBuf> {
    let mut wide: *mut u16 = std::ptr::null_mut();
    // SAFETY: the out pointer is valid for the call; the string it receives is freed below.
    let hr = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_Profile,
            KF_FLAG_DEFAULT as u32,
            std::ptr::null_mut(),
            &mut wide,
        )
    };
    if hr < 0 || wide.is_null() {
        return None;
    }
    let mut len = 0;
    // SAFETY: a successful call returns a NUL-terminated string.
    while unsafe { *wide.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: `len` characters are initialised, as counted above.
    let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(wide, len) });
    // SAFETY: the shell allocated this with the COM allocator and we own it.
    unsafe { CoTaskMemFree(wide as *const _) };
    Some(PathBuf::from(text))
}
