//! Documents that are a zip of XML parts — Word, Excel and PowerPoint files, and EPUB books — whose
//! properties live in one small part. Spec: Core/Services/Metadata/ZipDocumentCodec.cs.
//!
//! The whole container is written out again with that one part changed. Every other part is
//! copied entry for entry, in the same order, and the digest is over each one's name and
//! *uncompressed* bytes: recompressing an entry may produce different bytes on disk for the same
//! content, and it is the content that must not change.
//!
//! A document is somebody else's zip, so it is bounded like one: an entry count and a total size
//! past which it is refused rather than inflated into memory or onto disk.

use std::collections::BTreeMap;
use std::io::{Read, SeekFrom, Write};

use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::codec::{Destination, MetadataError, MetadataResult, Source, finish};
use super::fields::MetadataField;
use super::models::{MetadataDocument, MetadataEdit};
use super::xml::{Document, Element, Node, Scope};
use crate::cancel::CancelToken;

const MAX_ENTRIES: usize = 100_000;
const MAX_TOTAL_BYTES: u64 = 4 << 30;
const MAX_PART_BYTES: u64 = 16 << 20;

pub(crate) fn not_document() -> MetadataError {
    MetadataError::format("This is not a readable document of its kind, so it was left as it is.")
}

/// What one kind of zip document keeps where.
pub(crate) trait ZipDocument {
    /// The entry that holds the properties, or None when the document has none. `Format` when
    /// the container is not a document of this kind at all.
    fn properties_part(
        &self,
        archive: &mut ZipArchive<&mut dyn Source>,
    ) -> MetadataResult<Option<String>>;

    fn read_part(&self, part: &Document) -> BTreeMap<MetadataField, String>;

    /// Applies `edit` to the part in place.
    fn edit_part(&self, part: &mut Document, edit: &MetadataEdit) -> MetadataResult<()>;

    /// What in the properties part is *not* a property, for the digest: an EPUB's part also
    /// lists the book's files and their reading order.
    fn structure_of(&self, _part: &Document) -> String {
        String::new()
    }

    fn fields(&self) -> &[MetadataField];
}

pub(crate) fn open(source: &mut dyn Source) -> MetadataResult<ZipArchive<&mut dyn Source>> {
    source.seek(SeekFrom::Start(0))?;
    // Not a zip at all — which is what a password-protected Office file is.
    let archive = ZipArchive::new(source).map_err(|_| not_document())?;
    if archive.len() > MAX_ENTRIES {
        return Err(not_document());
    }
    let total: u64 = (0..archive.len())
        .filter_map(|i| archive.by_index_data(i).ok().map(|e| e.size()))
        .sum();
    if total > MAX_TOTAL_BYTES {
        return Err(not_document());
    }
    Ok(archive)
}

pub(crate) fn has_entry(archive: &mut ZipArchive<&mut dyn Source>, name: &str) -> bool {
    archive.index_for_name(name).is_some()
}

pub(crate) fn load(
    archive: &mut ZipArchive<&mut dyn Source>,
    name: &str,
) -> MetadataResult<Vec<u8>> {
    let mut entry = archive.by_name(name).map_err(|_| not_document())?;
    if entry.size() > MAX_PART_BYTES {
        return Err(not_document());
    }
    let mut buffer = Vec::with_capacity(entry.size() as usize);
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let read = entry.read(&mut chunk).map_err(|_| not_document())?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() as u64 > MAX_PART_BYTES {
            return Err(not_document());
        }
    }
    Ok(buffer)
}

/// Parsed with the document type declaration never processed: a declaration is how hostile XML
/// reaches outside itself, and nothing here needs what one says.
pub(crate) fn parse(xml: &[u8]) -> MetadataResult<Document> {
    if xml.len() as u64 > MAX_PART_BYTES {
        return Err(not_document());
    }
    let text = std::str::from_utf8(xml).map_err(|_| not_document())?;
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    Document::parse(text).ok_or_else(not_document)
}

/// Sets an element's text, adding it under `parent` when missing and removing it when the value
/// is empty. `scope` is the parent's own scope.
pub(crate) fn set_element(
    parent: &mut Element,
    scope: &Scope,
    namespace: &str,
    local: &str,
    value: &str,
) {
    let existing = parent.find_children(scope, namespace, local);
    if value.is_empty() {
        parent.remove_children(scope, namespace, local);
        return;
    }

    let Some(&first) = existing.first() else {
        parent.append_child(scope, namespace, local, Element::with_text("", value));
        return;
    };

    parent.child_mut(first).set_text(value);
    for &extra in existing.iter().skip(1).rev() {
        parent.children.remove(extra);
    }
}

pub(crate) fn read(
    document: &dyn ZipDocument,
    family: super::fields::MetadataFamily,
    source: &mut dyn Source,
) -> MetadataResult<MetadataDocument> {
    let mut archive = open(source)?;
    let values = match document.properties_part(&mut archive)? {
        Some(name) => document.read_part(&parse(&load(&mut archive, &name)?)?),
        None => BTreeMap::new(),
    };
    Ok(MetadataDocument::new(family, values))
}

pub(crate) fn write(
    document: &dyn ZipDocument,
    source: &mut dyn Source,
    destination: &mut dyn Destination,
    edit: &MetadataEdit,
    cancel: &CancelToken,
) -> MetadataResult<()> {
    let mut archive = open(source)?;
    let name = document.properties_part(&mut archive)?.ok_or_else(|| {
        MetadataError::format(
            "This document has no properties part to write to, so it was left as it is.",
        )
    })?;

    let mut part = parse(&load(&mut archive, &name)?)?;
    let applied = if edit.remove_all {
        let mut whole = edit.clone();
        whole.changes = document
            .fields()
            .iter()
            .map(|f| (*f, edit.changes.get(f).cloned().unwrap_or_default()))
            .collect();
        whole
    } else {
        edit.clone()
    };
    document.edit_part(&mut part, &applied)?;
    let rewritten = part.serialize(true).into_bytes();

    let mut output = ZipWriter::new(destination);
    for index in 0..archive.len() {
        cancel.check()?;

        let (entry_name, modified) = {
            let entry = archive.by_index_data(index).map_err(|_| not_document())?;
            (
                entry.name().map_err(|_| not_document())?.into_owned(),
                entry.last_modified(),
            )
        };

        // An EPUB's first entry names its type and must be stored, not compressed, or readers
        // that sniff the first bytes of the file stop recognising it.
        let mut options =
            SimpleFileOptions::default().compression_method(if entry_name == "mimetype" {
                CompressionMethod::Stored
            } else {
                CompressionMethod::Deflated
            });
        if let Some(modified) = modified {
            options = options.last_modified_time(modified);
        }
        output.start_file(entry_name.as_str(), options)?;

        if entry_name == name {
            output.write_all(&rewritten)?;
        } else {
            let mut from = archive.by_index(index).map_err(|_| not_document())?;
            std::io::copy(&mut from, &mut output).map_err(|_| not_document())?;
        }
    }

    let destination = output.finish()?;
    destination.flush()?;
    Ok(())
}

pub(crate) fn payload_digest(
    document: &dyn ZipDocument,
    source: &mut dyn Source,
) -> MetadataResult<Vec<u8>> {
    let mut archive = open(source)?;
    let name = document.properties_part(&mut archive)?;

    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; super::codec::COPY_BUFFER];

    for index in 0..archive.len() {
        let entry_name = archive
            .by_index_data(index)
            .map_err(|_| not_document())?
            .name()
            .map_err(|_| not_document())?
            .into_owned();
        hash.update(entry_name.as_bytes());
        hash.update([0]);

        if Some(&entry_name) == name.as_ref() {
            let part = parse(&load(&mut archive, &entry_name)?)?;
            hash.update(document.structure_of(&part).as_bytes());
            continue;
        }

        let mut content = archive.by_index(index).map_err(|_| not_document())?;
        loop {
            let read = content.read(&mut buffer).map_err(|_| not_document())?;
            if read == 0 {
                break;
            }
            hash.update(&buffer[..read]);
        }
    }

    Ok(finish(hash))
}

impl From<zip::result::ZipError> for MetadataError {
    fn from(e: zip::result::ZipError) -> MetadataError {
        match e {
            zip::result::ZipError::Io(io) => MetadataError::Io(io),
            _ => not_document(),
        }
    }
}

/// The serialization of one element, for `structure_of`.
pub(crate) fn element_text(element: &Element) -> String {
    let mut out = String::new();
    super::xml::write_element(element, &mut out);
    out
}

pub(crate) fn is_element(node: &Node) -> Option<&Element> {
    match node {
        Node::Element(e) => Some(e),
        _ => None,
    }
}
