//! EPUB books: the metadata block of the package document.
//! Spec: Core/Services/Metadata/ZipDocumentCodec.cs (`EpubCodec`).

use std::collections::BTreeMap;

use zip::ZipArchive;

use super::codec::{Destination, MetadataCodec, MetadataResult, Source};
use super::fields::{MetadataFamily, MetadataField, MetadataFields};
use super::models::{MetadataDocument, MetadataEdit};
use super::xml::{Document, Element, Scope};
use super::zip_document::{
    self, ZipDocument, element_text, has_entry, is_element, load, not_document, parse, set_element,
};
use crate::cancel::CancelToken;

const OPF_NS: &str = "http://www.idpf.org/2007/opf";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";
const CONTAINER_NS: &str = "urn:oasis:names:tc:opendocument:xmlns:container";

const ELEMENTS: [(MetadataField, &str); 5] = [
    (MetadataField::Title, "title"),
    (MetadataField::Author, "creator"),
    (MetadataField::Comment, "description"),
    (MetadataField::Publisher, "publisher"),
    (MetadataField::Copyright, "rights"),
];

const FIELDS: [MetadataField; 6] = [
    MetadataField::Title,
    MetadataField::Author,
    MetadataField::Comment,
    MetadataField::Publisher,
    MetadataField::Copyright,
    MetadataField::Keywords,
];

pub struct EpubCodec;

/// The `<metadata>` under the package root, with the scope it is read in.
fn metadata(part: &Document) -> Option<(&Element, Scope)> {
    let root = part.root()?;
    let scope = Scope::root().child(root);
    let index = *root.find_children(&scope, OPF_NS, "metadata").first()?;
    let element = is_element(&root.children[index])?;
    let own = scope.child(element);
    Some((element, own))
}

fn metadata_mut(part: &mut Document) -> Option<(&mut Element, Scope)> {
    let root = part.root_mut()?;
    let scope = Scope::root().child(root);
    let index = *root.find_children(&scope, OPF_NS, "metadata").first()?;
    let element = root.child_mut(index);
    let own = scope.child(element);
    Some((element, own))
}

impl ZipDocument for EpubCodec {
    /// The package document, which the container file names — it has no fixed path.
    fn properties_part(
        &self,
        archive: &mut ZipArchive<&mut dyn Source>,
    ) -> MetadataResult<Option<String>> {
        if !has_entry(archive, "META-INF/container.xml") {
            return Err(not_document());
        }

        let container = parse(&load(archive, "META-INF/container.xml")?)?;
        let mut path: Option<String> = None;
        container.visit(|element, scope| {
            if path.is_none()
                && element.is(scope, CONTAINER_NS, "rootfile")
                && let Some(full) = element.attribute(scope, "", "full-path")
                && !full.is_empty()
            {
                path = Some(full.to_string());
            }
        });

        match path {
            Some(p) if has_entry(archive, &p) => Ok(Some(p)),
            _ => Err(not_document()),
        }
    }

    fn read_part(&self, part: &Document) -> BTreeMap<MetadataField, String> {
        let mut values = BTreeMap::new();
        let Some((metadata, scope)) = metadata(part) else {
            return values;
        };

        for (field, local) in ELEMENTS {
            if let Some(element) = metadata.first_child(&scope, DC_NS, local)
                && let Some(value) = MetadataFields::try_normalize(field, &element.text())
                && !value.is_empty()
            {
                values.insert(field, value);
            }
        }

        // A book's subjects are one element each; a semicolon inside one would split it on the
        // way back, so it is written out as the separator it will be read as.
        let subjects: Vec<String> = metadata
            .find_children(&scope, DC_NS, "subject")
            .into_iter()
            .filter_map(|i| is_element(&metadata.children[i]))
            .map(|s| s.text().replace(';', ",").trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if let Some(keywords) =
            MetadataFields::try_normalize(MetadataField::Keywords, &subjects.join(";"))
            && !keywords.is_empty()
        {
            values.insert(MetadataField::Keywords, keywords);
        }

        values
    }

    fn edit_part(&self, part: &mut Document, edit: &MetadataEdit) -> MetadataResult<()> {
        let (metadata, scope) = metadata_mut(part).ok_or_else(not_document)?;

        for (field, value) in &edit.changes {
            if let Some((_, local)) = ELEMENTS.iter().find(|(f, _)| f == field) {
                // A book must have a title; an emptied one is left as an empty element.
                let titled = metadata.find_children(&scope, DC_NS, local);
                if *field == MetadataField::Title
                    && value.is_empty()
                    && let Some(&first) = titled.first()
                {
                    metadata.child_mut(first).set_text("");
                } else {
                    set_element(metadata, &scope, DC_NS, local, value);
                }
            } else if *field == MetadataField::Keywords {
                metadata.remove_children(&scope, DC_NS, "subject");
                for subject in MetadataFields::split_list(value) {
                    metadata.append_child(
                        &scope,
                        DC_NS,
                        "subject",
                        Element::with_text("", &subject),
                    );
                }
            }
        }
        Ok(())
    }

    /// Everything in the package document but its metadata: the list of the book's files and the
    /// order they are read in.
    fn structure_of(&self, part: &Document) -> String {
        let Some(root) = part.root() else {
            return String::new();
        };
        let scope = Scope::root().child(root);
        root.child_elements()
            .filter(|e| !e.is(&scope.child(e), OPF_NS, "metadata"))
            .map(element_text)
            .collect()
    }

    fn fields(&self) -> &[MetadataField] {
        &FIELDS
    }
}

impl MetadataCodec for EpubCodec {
    fn id(&self) -> &'static str {
        "epub"
    }

    fn family(&self) -> MetadataFamily {
        MetadataFamily::Document
    }

    fn fields(&self) -> &[MetadataField] {
        &FIELDS
    }

    fn read(&self, source: &mut dyn Source) -> MetadataResult<MetadataDocument> {
        zip_document::read(self, MetadataFamily::Document, source)
    }

    fn write(
        &self,
        source: &mut dyn Source,
        destination: &mut dyn Destination,
        edit: &MetadataEdit,
        cancel: &CancelToken,
    ) -> MetadataResult<()> {
        zip_document::write(self, source, destination, edit, cancel)
    }

    fn payload_digest(&self, source: &mut dyn Source) -> MetadataResult<Vec<u8>> {
        zip_document::payload_digest(self, source)
    }
}
