//! Word, Excel and PowerPoint files: the core properties part every one of them carries.
//! Spec: Core/Services/Metadata/ZipDocumentCodec.cs (`OpenXmlCodec`).

use std::collections::BTreeMap;

use zip::ZipArchive;

use super::codec::{Destination, MetadataCodec, MetadataResult, Source};
use super::fields::{MetadataFamily, MetadataField, MetadataFields};
use super::models::{MetadataDocument, MetadataEdit};
use super::xml::{Document, Scope};
use super::zip_document::{self, ZipDocument, has_entry, not_document, set_element};
use crate::cancel::CancelToken;

const PART: &str = "docProps/core.xml";
const CP_NS: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";

/// Which element each field is, in the core properties part.
const ELEMENTS: [(MetadataField, &str, &str); 6] = [
    (MetadataField::Title, DC_NS, "title"),
    (MetadataField::Author, DC_NS, "creator"),
    (MetadataField::Subject, DC_NS, "subject"),
    (MetadataField::Keywords, CP_NS, "keywords"),
    (MetadataField::Comment, DC_NS, "description"),
    (MetadataField::Category, CP_NS, "category"),
];

const FIELDS: [MetadataField; 6] = [
    MetadataField::Title,
    MetadataField::Author,
    MetadataField::Subject,
    MetadataField::Keywords,
    MetadataField::Comment,
    MetadataField::Category,
];

pub struct OpenXmlCodec;

impl ZipDocument for OpenXmlCodec {
    fn properties_part(
        &self,
        archive: &mut ZipArchive<&mut dyn Source>,
    ) -> MetadataResult<Option<String>> {
        if !has_entry(archive, "[Content_Types].xml") {
            return Err(not_document());
        }
        Ok(has_entry(archive, PART).then(|| PART.to_string()))
    }

    fn read_part(&self, part: &Document) -> BTreeMap<MetadataField, String> {
        let mut values = BTreeMap::new();
        let Some(root) = part.root() else {
            return values;
        };
        let scope = Scope::root().child(root);

        for (field, namespace, local) in ELEMENTS {
            let Some(element) = root.first_child(&scope, namespace, local) else {
                continue;
            };
            let raw = element.text();
            if raw.trim().is_empty() {
                continue;
            }
            if let Some(value) = MetadataFields::try_normalize(field, &raw)
                && !value.is_empty()
            {
                values.insert(field, value);
            }
        }

        values
    }

    fn edit_part(&self, part: &mut Document, edit: &MetadataEdit) -> MetadataResult<()> {
        let root = part.root_mut().ok_or_else(not_document)?;
        let scope = Scope::root().child(root);
        for (field, value) in &edit.changes {
            if let Some((_, namespace, local)) = ELEMENTS.iter().find(|(f, _, _)| f == field) {
                set_element(root, &scope, namespace, local, value);
            }
        }

        // Who last saved it is the one property here nobody types and everybody means by "all".
        if edit.remove_all {
            set_element(root, &scope, CP_NS, "lastModifiedBy", "");
        }
        Ok(())
    }

    fn fields(&self) -> &[MetadataField] {
        &FIELDS
    }
}

impl MetadataCodec for OpenXmlCodec {
    fn id(&self) -> &'static str {
        "openxml"
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
