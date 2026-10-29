//! The bookmark repository with the one rule the menu's greying is only a courtesy for.
//! Spec: Core/Services/BookmarkService.cs (minus the async facade: callers thread as they like).

use std::path::Path;

use super::{Bookmark, BookmarkRepository, Result};
use crate::archives::ArchivePath;

pub struct BookmarkService {
    repository: BookmarkRepository,
}

impl BookmarkService {
    pub fn new(repository: BookmarkRepository) -> BookmarkService {
        BookmarkService { repository }
    }

    pub fn get_all(&self) -> Result<Vec<Bookmark>> {
        self.repository.get_all()
    }

    /// Adds a bookmark; false if it was already there — or if the path is inside an archive.
    ///
    /// **A path inside an archive is refused here, not only in the menu.** The bookmark table is
    /// keyed by `PathKey`, and a virtual path canonicalizes perfectly happily — which is the trap:
    /// `is_under` then places it strictly inside the archive's own containing folder, so one such
    /// row would make every subtree range scan over that folder start returning archive interiors.
    pub fn add(&self, path: &str, is_directory: bool) -> Result<bool> {
        if ArchivePath::parse(path, |p| Path::new(p).is_file()).is_some() {
            return Ok(false);
        }
        self.repository.add(path, is_directory)
    }

    pub fn remove(&self, path: &str) -> Result<()> {
        self.repository.remove(path)
    }

    pub fn exists(&self, path: &str) -> Result<bool> {
        self.repository.exists(path)
    }
}
