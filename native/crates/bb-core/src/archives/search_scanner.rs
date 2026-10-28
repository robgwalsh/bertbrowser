//! Runs a parsed query over an archive's already-loaded index. Spec: Core/Services/Archives/
//! ArchiveSearchScanner.cs.
//!
//! **It reuses `SearchQuery::matches` verbatim** by building a `SearchCandidate` per node, which
//! is the whole reason this is a short walk rather than a second query engine: `ext:`, `size:`,
//! `re:`, `OR`, `!`, brackets all work in here for free, and `dm:` correctly matches nothing for an
//! entry the container gave no timestamp. Nothing here touches the index or the disk: searching
//! inside a container is answered from what the listing already read, which is what keeps the
//! hard invariant that a virtual path never reaches a `PathKey`-keyed table.

use bb_paths::{fold_upper, split_last};

use super::{ArchiveIndex, ArchiveNode, ArchivePath};
use crate::FileTime;
use crate::cancel::{CancelToken, Cancelled};
use crate::data::SearchHit;
use crate::search::{SearchCandidate, SearchQuery};

pub struct ArchiveSearchScanner;

impl ArchiveSearchScanner {
    /// Every entry under `relative_to` that the query matches, as search hits with virtual paths.
    /// `query` None lists every entry instead of matching one, which is what the flat branch view
    /// asks for inside an open container. `include_directories` false lists files only, as the
    /// flat view does; the walk still descends, since what is emitted and what is recursed into
    /// are separate.
    pub fn search(
        index: &ArchiveIndex,
        archive_file: &str,
        relative_to: &str,
        query: Option<&SearchQuery>,
        limit: usize,
        cancel: &CancelToken,
        include_directories: bool,
    ) -> Result<Vec<SearchHit>, Cancelled> {
        let mut hits = Vec::new();
        if !index.ok() {
            return Ok(hits);
        }

        // A content query has no answer in here, and the failure mode is the dangerous kind rather
        // than the obvious one. An entry has no file on disk to open, so its candidate carries no
        // content, and an unread candidate counts as a *possible* match by design. Run the walk
        // anyway and every entry in the container comes back as a hit. The grammar refuses
        // `content: in:archives` before it gets here; this is the guard that does not depend on
        // two callers remembering.
        if query.is_some_and(|q| q.needs_content()) {
            return Ok(hits);
        }

        let Some(start) = index.find(relative_to) else {
            return Ok(hits);
        };

        let mut stack: Vec<&ArchiveNode> = index.children_of(start);

        while let Some(node) = stack.pop() {
            if hits.len() >= limit {
                break;
            }
            cancel.check()?;

            if node.is_directory {
                stack.extend(index.children_of(node));
            }
            if !include_directories && node.is_directory {
                continue;
            }

            let virtual_path = ArchivePath::compose(archive_file, &node.path);

            let Some(query) = query else {
                hits.push(Self::hit(virtual_path, node, relative_to));
                continue;
            };

            let name_key = fold_upper(&node.name);
            let path_key = fold_upper(&virtual_path);
            let candidate = SearchCandidate {
                name_key: &name_key,
                path_key: &path_key,
                is_directory: node.is_directory,
                size_bytes: node.size_bytes,
                modified_utc: node.modified.unwrap_or(FileTime::UNKNOWN),
                // Nothing inside an archive is hidden: the listing takes the same view, and for the
                // same reason, the attribute a container carries means different things depending
                // on which tool wrote it. The rest of the mask is absent for that same reason, so
                // is:readonly and friends answer "no" here rather than guessing from a Unix mode.
                // A container directory records one timestamp, and it is the modified one, so
                // there is no creation date either.
                hidden: false,
                attributes: 0,
                created_utc: FileTime::UNKNOWN,
                content: None,
            };

            if !query.matches(&candidate) {
                continue;
            }
            hits.push(Self::hit(virtual_path, node, relative_to));
        }

        Ok(hits)
    }

    fn hit(virtual_path: String, node: &ArchiveNode, relative_to: &str) -> SearchHit {
        SearchHit {
            display_path: virtual_path,
            relative_dir_display: Self::relative_dir_display(&node.path, relative_to),
            name: node.name.clone(),
            is_directory: node.is_directory,
            size_bytes: node.size_bytes,
            modified_utc: node.modified.unwrap_or(FileTime::UNKNOWN),
            hidden: false,
            attributes: 0,
            created_utc: FileTime::UNKNOWN,
            content_match: None,
        }
    }

    /// The folder an entry sits in, relative to the search root: the Folder column.
    fn relative_dir_display(entry_path: &str, relative_to: &str) -> String {
        let parent = split_last(entry_path).0;
        if relative_to.is_empty() {
            return parent.to_string();
        }
        let prefix = format!("{}\\", relative_to.trim_end_matches('\\'));
        if parent.len() >= prefix.len()
            && parent.is_char_boundary(prefix.len())
            && parent[..prefix.len()].eq_ignore_ascii_case(&prefix)
        {
            parent[prefix.len()..].to_string()
        } else {
            parent.to_string()
        }
    }
}
