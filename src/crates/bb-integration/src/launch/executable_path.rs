//! Resolves a program name — `wt.exe`, `code`, or a path the user typed into a custom command — to
//! the absolute path of a file that exists, the way Windows searches `PATH`.
//!
//! This exists because launches go through the shell, which reports nothing back: no handle, no
//! exit code, no signal that the target was even found. "Open in Terminal" used to discover that
//! Windows Terminal was not installed by letting the launch *throw* and falling through to
//! PowerShell; with no throw, that fallback would silently do nothing. Resolving first turns "is it
//! installed?" back into a question with an answer: None is the fall-through signal.
//!
//! It is also the safer order of operations. Handing a bare `code` to the shell lets an *elevated*
//! process resolve a name through a search path that may contain a directory some lesser account
//! can write to; resolving to an absolute path first, here, decides what runs.
//!
//! Pure: the environment and the "does this exist" probe are arguments, so the search order is
//! tested rather than inferred from whatever is installed. Spec: Core/Services/ExecutablePath.cs.

use std::io;

use super::win_path::{combine, full_path, has_extension, is_fully_qualified, is_rooted};

pub struct ExecutablePath;

impl ExecutablePath {
    /// What Windows uses when `PATHEXT` is unset.
    pub const DEFAULT_PATH_EXT: &'static str = ".COM;.EXE;.BAT;.CMD";

    /// The absolute path `program` resolves to, or None if nothing matches.
    ///
    /// `program` is a bare name (`code`), a name with an extension (`wt.exe`) or a full path;
    /// `path_variable` the `PATH` value, `;`-separated; `path_ext` the `PATHEXT` value, falling
    /// back to [`DEFAULT_PATH_EXT`](Self::DEFAULT_PATH_EXT) when blank. `exists` probes for a
    /// file; an error from it costs that one candidate, never the search.
    pub fn resolve(
        program: Option<&str>,
        path_variable: Option<&str>,
        path_ext: Option<&str>,
        exists: impl Fn(&str) -> io::Result<bool>,
    ) -> Option<String> {
        let program = program?.trim().trim_matches('"');
        if program.is_empty() {
            return None;
        }

        let extensions = parse_extensions(path_ext);

        // Something that already names a location is not a PATH lookup — take it or leave it, but
        // never fall back to searching, or "C:\gone\tool.exe" could start a different tool.
        if names_a_location(program) {
            // ...and it has to say *which* location. A relative path resolves against the current
            // directory, which for this process is wherever it happened to be started, and that
            // must not be what decides which program runs.
            if !is_fully_qualified(program) {
                return None;
            }
            return first_that_exists(program, &extensions, &exists);
        }

        parse_search_path(path_variable)
            .into_iter()
            .find_map(|directory| {
                first_that_exists(&combine(&directory, program), &extensions, &exists)
            })
    }

    /// [`resolve`](Self::resolve) over the process's own `PATH` and `PATHEXT` and the real disk.
    pub fn resolve_on_disk(program: &str) -> Option<String> {
        let path = std::env::var("PATH").ok();
        let path_ext = std::env::var("PATHEXT").ok();
        Self::resolve(
            Some(program),
            path.as_deref(),
            path_ext.as_deref(),
            file_exists,
        )
    }
}

/// Whether a path names a file (not a directory), as `File.Exists` answered.
pub(crate) fn file_exists(path: &str) -> io::Result<bool> {
    match std::fs::metadata(path) {
        Ok(m) => Ok(m.is_file()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// A name carrying its own extension is tried only as written; a bare one is tried against each
/// `PATHEXT` in turn, which is what finds `code.cmd` for `code`. Deliberately not both: appending
/// extensions to a name that has one would let `wt.exe.bat` answer for `wt.exe`.
fn first_that_exists(
    candidate: &str,
    extensions: &[String],
    exists: &impl Fn(&str) -> io::Result<bool>,
) -> Option<String> {
    if has_extension(candidate) {
        return safely(candidate, exists).then(|| full_path(candidate));
    }

    extensions.iter().find_map(|extension| {
        let with_extension = format!("{candidate}{extension}");
        safely(&with_extension, exists).then(|| full_path(&with_extension))
    })
}

/// The probe is caller-supplied and reaches the filesystem; a path it dislikes costs that one
/// candidate rather than the whole resolution.
fn safely(candidate: &str, exists: &impl Fn(&str) -> io::Result<bool>) -> bool {
    exists(candidate).unwrap_or(false)
}

fn names_a_location(program: &str) -> bool {
    program.contains('\\') || program.contains('/') || is_rooted(program)
}

fn parse_extensions(path_ext: Option<&str>) -> Vec<String> {
    let source = match path_ext {
        Some(p) if !p.trim().is_empty() => p,
        _ => ExecutablePath::DEFAULT_PATH_EXT,
    };
    let extensions: Vec<String> = source
        .split(';')
        .map(|raw| raw.trim().trim_matches('"'))
        .filter(|e| !e.is_empty())
        .map(|e| {
            if e.starts_with('.') {
                e.to_string()
            } else {
                format!(".{e}")
            }
        })
        .collect();

    if extensions.is_empty() {
        parse_extensions(Some(ExecutablePath::DEFAULT_PATH_EXT))
    } else {
        extensions
    }
}

fn parse_search_path(path_variable: Option<&str>) -> Vec<String> {
    let Some(path_variable) = path_variable else {
        return Vec::new();
    };
    path_variable
        .split(';')
        // PATH entries are commonly quoted when they contain spaces.
        .map(|raw| raw.trim().trim_matches('"').trim())
        .filter(|d| !d.is_empty())
        .map(str::to_string)
        .collect()
}
