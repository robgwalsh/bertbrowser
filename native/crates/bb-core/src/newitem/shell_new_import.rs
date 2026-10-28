//! Turns what the registry says under `ShellNew` into the app's own list of new-file types.
//! Spec: Core/Services/NewItem/ShellNewImport.cs.
//!
//! This is the half of the ShellNew read that has behaviour worth asserting, kept in Core so it can
//! be tested; the shell half only opens keys and hands the values over.
//!
//! **`Command` entries are dropped.** They name a program to run — it is how Shortcut and
//! Briefcase work — and honouring one would put a registry-supplied command line through the
//! process launcher, past the single chokepoint this app starts programs from. A "New" menu is not
//! worth that.

use std::collections::HashSet;

use super::NewFileTemplate;
use crate::rename::RenamePattern;
use crate::rename::paths::{combine, is_rooted};

/// How Windows says a new file of some type should be produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellNewKind {
    /// An empty file.
    NullFile,
    /// Copy a template file, named by `ShellNewEntry::file_name`.
    FileName,
    /// Write `ShellNewEntry::data` as the file's contents.
    Data,
    /// Run a program to produce it. Never honoured.
    Command,
}

/// The raw ShellNew values for one extension, exactly as the registry holds them. Nothing here has
/// been interpreted yet, which is the point: it is what lets the interpretation be tested in a
/// crate that cannot read a registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellNewEntry {
    /// Including the dot, e.g. ".txt".
    pub extension: String,
    /// The friendly type name from the extension's ProgID, which may still be an unresolved
    /// indirect resource string, or None when there wasn't one.
    pub label: Option<String>,
    pub kind: ShellNewKind,
    /// The template named by a `FileName` entry.
    pub file_name: Option<String>,
    /// The bytes held by a `Data` entry.
    pub data: Option<Vec<u8>>,
}

impl ShellNewEntry {
    pub fn new(extension: &str, label: Option<&str>, kind: ShellNewKind) -> ShellNewEntry {
        ShellNewEntry {
            extension: extension.to_string(),
            label: label.map(str::to_string),
            kind,
            file_name: None,
            data: None,
        }
    }
}

pub struct ShellNewImport;

impl ShellNewImport {
    /// The types worth offering, in the order given. `resolve_indirect_string` resolves an
    /// `@file,-id` label or answers None; `file_exists` says whether a template is really there;
    /// `template_roots` are the folders a bare template name is resolved against; `save_data`
    /// writes a `Data` entry's bytes out to a file and returns its path, or None if it could not —
    /// the file writing stays with the caller while the decision to do it stays here.
    pub fn to_templates(
        entries: &[ShellNewEntry],
        resolve_indirect_string: impl Fn(&str) -> Option<String>,
        file_exists: impl Fn(&str) -> bool,
        template_roots: &[String],
        mut save_data: impl FnMut(&str, &[u8]) -> Option<String>,
    ) -> Vec<NewFileTemplate> {
        let mut templates = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        for entry in entries {
            // A registry-supplied command line is never worth a menu entry.
            if entry.kind == ShellNewKind::Command {
                continue;
            }
            if !is_usable_extension(&entry.extension) {
                continue;
            }
            if !seen.insert(entry.extension.to_lowercase()) {
                continue;
            }

            let template_path = match entry.kind {
                ShellNewKind::FileName => {
                    // A type whose template is not installed would only ever refuse; leave it
                    // out.
                    match Self::resolve_template(
                        entry.file_name.as_deref(),
                        template_roots,
                        &file_exists,
                    ) {
                        Some(path) => Some(path),
                        None => continue,
                    }
                }
                ShellNewKind::Data => match entry.data.as_deref().filter(|d| !d.is_empty()) {
                    Some(data) => match save_data(&entry.extension, data) {
                        Some(path) => Some(path),
                        None => continue,
                    },
                    None => None,
                },
                ShellNewKind::NullFile | ShellNewKind::Command => None,
            };

            templates.push(NewFileTemplate {
                label: Self::label_for(entry, &resolve_indirect_string),
                extension: entry.extension.to_lowercase(),
                template_path,
                enabled: true,
            });
        }

        templates
    }

    /// What the menu should call this type.
    pub fn label_for(
        entry: &ShellNewEntry,
        resolve_indirect_string: &impl Fn(&str) -> Option<String>,
    ) -> String {
        let mut label = entry.label.clone();

        if let Some(text) = &label
            && !text.is_empty()
            && Self::is_indirect_string(text)
        {
            // An unresolvable resource reference must never reach the menu as its own raw text.
            label = resolve_indirect_string(text).filter(|r| !r.trim().is_empty());
        }

        match label {
            Some(text) if !text.trim().is_empty() => text.trim().to_string(),
            _ => Self::fallback_label(&entry.extension),
        }
    }

    /// Whether a label is a resource reference rather than real text.
    pub fn is_indirect_string(label: &str) -> bool {
        label.starts_with('@')
    }

    /// ".txt" becomes "TXT File" — what Explorer shows for a type it has no name for.
    pub fn fallback_label(extension: &str) -> String {
        format!("{} File", extension.trim_start_matches('.').to_uppercase())
    }

    /// Where a ShellNew template really is, or None if it is nowhere. A bare name is resolved
    /// against the template folders; a rooted one is taken as given.
    pub fn resolve_template(
        file_name: Option<&str>,
        roots: &[String],
        file_exists: &impl Fn(&str) -> bool,
    ) -> Option<String> {
        let file_name = file_name.filter(|f| !f.is_empty())?;

        if is_rooted(file_name) {
            return file_exists(file_name).then(|| file_name.to_string());
        }

        roots
            .iter()
            .map(|root| combine(root, file_name))
            .find(|candidate| file_exists(candidate))
    }

    /// Existing entries keep their place and their settings; an import only adds types for
    /// extensions the user does not already have, so re-importing never disturbs the list.
    pub fn merge(
        existing: &[NewFileTemplate],
        imported: &[NewFileTemplate],
    ) -> Vec<NewFileTemplate> {
        let mut known: HashSet<String> = existing
            .iter()
            .map(|t| t.extension.to_lowercase())
            .collect();
        let mut merged: Vec<NewFileTemplate> = existing.to_vec();
        for template in imported {
            if known.insert(template.extension.to_lowercase()) {
                merged.push(template.clone());
            }
        }
        merged
    }
}

/// An extension has to be able to end a legal file name, or the type it names could never produce
/// one.
fn is_usable_extension(extension: &str) -> bool {
    extension.len() > 1
        && extension.starts_with('.')
        && !extension[1..].contains('.')
        && RenamePattern::validate(&format!("x{extension}")).is_none()
}
