//! An XMP packet — the XML description many programs write beside a picture's EXIF.
//! Spec: Core/Services/Metadata/XmpPacket.cs.
//!
//! **Untrusted XML**, so it is read with DTDs prohibited and a size limit: a picture from the
//! internet must not be able to make this app fetch a URL or expand a billion entities.
//!
//! **When in doubt, the answer is that a location is there.** A packet this cannot parse but
//! which mentions GPS is reported as holding one, and removing it is then refused — the opposite
//! mistake is telling somebody a photograph is safe to share when it still says where they live.

use std::collections::BTreeMap;

use super::codec::{MetadataError, MetadataResult, index_of};
use super::fields::{MetadataField, MetadataFields};
use super::xml::{Document, Element, Node, Scope};

const EXIF_NS: &str = "http://ns.adobe.com/exif/1.0/";
const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";
const XMP_NS: &str = "http://ns.adobe.com/xap/1.0/";
const TIFF_NS: &str = "http://ns.adobe.com/tiff/1.0/";
const PHOTOSHOP_NS: &str = "http://ns.adobe.com/photoshop/1.0/";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const MAX_CHARACTERS: usize = 8 << 20;

pub struct XmpPacket;

impl XmpPacket {
    /// Whether the packet says where the picture was taken.
    pub fn has_location(packet: &[u8]) -> bool {
        if !mentions_gps(packet) {
            return false;
        }
        match parse(packet) {
            None => true,
            Some(document) => has_location_nodes(&document),
        }
    }

    /// The packet with its GPS properties taken out, or None when it had none. `Format` when it
    /// mentions a location in a form that cannot be parsed, and so cannot be shown to be gone.
    pub fn without_location(packet: &[u8]) -> MetadataResult<Option<Vec<u8>>> {
        if !mentions_gps(packet) {
            return Ok(None);
        }

        let unsafe_to_remove = || {
            MetadataError::format(
                "This picture's location is stored in a form that cannot be safely removed.",
            )
        };
        let mut document = parse(packet).ok_or_else(unsafe_to_remove)?;
        if !has_location_nodes(&document) {
            return Ok(None);
        }

        document.visit_mut(|element, scope| {
            element
                .attributes
                .retain(|(k, _)| !is_location(&scope.resolve_attribute(k)));
            element.children.retain(|node| match node {
                Node::Element(child) => {
                    !is_location(&scope.child(child).resolve_element(&child.name))
                }
                _ => true,
            });
        });

        let written = document.serialize(false).into_bytes();

        // Checked on the bytes that will be written, not on the tree they came from.
        if Self::has_location(&written) {
            return Err(unsafe_to_remove());
        }
        Ok(Some(written))
    }

    /// The packet with the same changes made to it that were made to the EXIF, or None when it
    /// did not need any or could not be parsed.
    ///
    /// **Only ever brought into step, never created.** Many programs — Windows among them — read
    /// a title from here before they look at EXIF, so a packet that already states one has to
    /// change with it, or the edit appears not to have happened everywhere but in this app. A
    /// picture with no packet is left without one: EXIF alone says the same thing to everybody.
    pub fn with_changes(
        packet: &[u8],
        changes: &BTreeMap<MetadataField, String>,
    ) -> Option<Vec<u8>> {
        if changes.is_empty() {
            return None;
        }
        let mut document = parse(packet)?;
        let descriptions = Descriptions::find(&document);
        if descriptions.is_empty() {
            return None;
        }

        for (field, value) in changes {
            let one: Vec<String> = if value.is_empty() {
                vec![]
            } else {
                vec![value.clone()]
            };
            match field {
                // The EXIF tag a title is written to is the one XMP calls the description, and
                // Windows writes a title to both. So a description that is already there follows
                // the title; one is not invented for a picture that has only a title.
                MetadataField::Title => {
                    descriptions.set_array(&mut document, DC_NS, "title", "Alt", &one, false);
                    descriptions.set_array(&mut document, DC_NS, "description", "Alt", &one, true);
                }
                MetadataField::Copyright => {
                    descriptions.set_array(&mut document, DC_NS, "rights", "Alt", &one, false);
                }
                MetadataField::Author => {
                    descriptions.set_array(&mut document, DC_NS, "creator", "Seq", &one, false);
                }
                MetadataField::Keywords => {
                    let values = MetadataFields::split_list(value);
                    descriptions.set_array(&mut document, DC_NS, "subject", "Bag", &values, false);
                }
                MetadataField::Comment => {
                    descriptions.set_array(
                        &mut document,
                        EXIF_NS,
                        "UserComment",
                        "Alt",
                        &one,
                        false,
                    );
                }
                MetadataField::Rating => {
                    descriptions.set_simple(&mut document, XMP_NS, "Rating", value, false);
                }
                MetadataField::CameraMake => {
                    descriptions.set_simple(&mut document, TIFF_NS, "Make", value, false);
                }
                MetadataField::CameraModel => {
                    descriptions.set_simple(&mut document, TIFF_NS, "Model", value, false);
                }
                MetadataField::DateTaken => {
                    let iso = match MetadataFields::try_parse_date(value) {
                        Some(date) => {
                            let c = date.civil();
                            format!(
                                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                                c.year, c.month, c.day, c.hour, c.minute, c.second
                            )
                        }
                        None => String::new(),
                    };
                    descriptions.set_simple(
                        &mut document,
                        EXIF_NS,
                        "DateTimeOriginal",
                        &iso,
                        false,
                    );
                    descriptions.set_simple(&mut document, PHOTOSHOP_NS, "DateCreated", &iso, true);
                }
                _ => {}
            }
        }

        Some(document.serialize(false).into_bytes())
    }
}

/// The paths to every `rdf:Description` in the document: a sequence of child indices from the
/// document's nodes, with the scope each one is read in.
struct Descriptions {
    paths: Vec<(Vec<usize>, Scope)>,
}

impl Descriptions {
    fn find(document: &Document) -> Descriptions {
        let mut paths = Vec::new();
        fn walk(
            nodes: &[Node],
            scope: &Scope,
            path: &mut Vec<usize>,
            out: &mut Vec<(Vec<usize>, Scope)>,
        ) {
            for (i, node) in nodes.iter().enumerate() {
                if let Node::Element(e) = node {
                    let own = scope.child(e);
                    path.push(i);
                    if e.is(&own, RDF_NS, "Description") {
                        out.push((path.clone(), own.clone()));
                    }
                    walk(&e.children, &own, path, out);
                    path.pop();
                }
            }
        }
        walk(&document.nodes, &Scope::root(), &mut Vec::new(), &mut paths);
        Descriptions { paths }
    }

    fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    fn element<'d>(document: &'d mut Document, path: &[usize]) -> &'d mut Element {
        let mut node = &mut document.nodes[path[0]];
        for &index in &path[1..] {
            node = match node {
                Node::Element(e) => &mut e.children[index],
                _ => unreachable!("a description path only passes through elements"),
            };
        }
        match node {
            Node::Element(e) => e,
            _ => unreachable!("a description is an element"),
        }
    }

    /// A property is written either as an attribute or as a child element, and a packet may say
    /// it in more than one description. Every statement of it goes; one comes back.
    fn clear(&self, document: &mut Document, namespace: &str, local: &str) -> bool {
        let mut found = false;
        for (path, scope) in &self.paths {
            let description = Self::element(document, path);
            found |= description.remove_attributes(scope, namespace, local);
            found |= description.remove_children(scope, namespace, local);
        }
        found
    }

    fn set_simple(
        &self,
        document: &mut Document,
        namespace: &str,
        local: &str,
        value: &str,
        only_if_present: bool,
    ) {
        let had = self.clear(document, namespace, local);
        if value.is_empty() || (only_if_present && !had) {
            return;
        }
        let (path, scope) = &self.paths[0];
        let description = Self::element(document, path);
        declare(description, scope, namespace);
        description.append_child(scope, namespace, local, Element::with_text("", value));
    }

    fn set_array(
        &self,
        document: &mut Document,
        namespace: &str,
        local: &str,
        kind: &str,
        values: &[String],
        only_if_present: bool,
    ) {
        let had = self.clear(document, namespace, local);
        if values.is_empty() || (only_if_present && !had) {
            return;
        }
        let (path, scope) = &self.paths[0];
        let description = Self::element(document, path);
        declare(description, scope, namespace);
        let scope = scope.child(description);
        let property = description.append_child(&scope, namespace, local, Element::new(""));
        let scope = scope.child(property);
        let array = property.append_child(&scope, RDF_NS, kind, Element::new(""));
        let scope = scope.child(array);
        for value in values {
            let mut item = Element::with_text("", value);
            if kind == "Alt" {
                let lang = scope
                    .qualify(XML_NS, "lang")
                    .unwrap_or_else(|| "xml:lang".to_string());
                item.attributes.push((lang, "x-default".to_string()));
            }
            array.append_child(&scope, RDF_NS, "li", item);
        }
    }
}

/// Gives a namespace its usual prefix where the packet has not declared one, so the result reads
/// `dc:title` rather than an invented `p1:title`.
fn declare(description: &mut Element, scope: &Scope, namespace: &str) {
    let prefix = match namespace {
        DC_NS => "dc",
        XMP_NS => "xmp",
        EXIF_NS => "exif",
        TIFF_NS => "tiff",
        PHOTOSHOP_NS => "photoshop",
        _ => return,
    };
    description.declare(scope, prefix, namespace);
}

fn is_location((namespace, local): &(String, &str)) -> bool {
    namespace == EXIF_NS && local.starts_with("GPS")
}

fn has_location_nodes(document: &Document) -> bool {
    let mut found = false;
    document.visit(|element, scope| {
        if found {
            return;
        }
        found = is_location(&scope.resolve_element(&element.name))
            || element
                .attributes
                .iter()
                .any(|(k, _)| is_location(&scope.resolve_attribute(k)));
    });
    found
}

/// A cheap test before the expensive one: no packet without these three letters has a GPS
/// property, in any encoding XMP allows.
fn mentions_gps(packet: &[u8]) -> bool {
    index_of(packet, b"GPS").is_some() || index_of(packet, b"G\0P\0S").is_some()
}

fn parse(packet: &[u8]) -> Option<Document> {
    let text = std::str::from_utf8(packet).ok()?;
    if text.len() > MAX_CHARACTERS {
        return None;
    }
    Document::parse(text.trim_end_matches(['\0', ' ', '\r', '\n', '\t']))
}
