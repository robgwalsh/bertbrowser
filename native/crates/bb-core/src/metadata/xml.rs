//! A small namespace-aware XML tree over quick-xml, standing in for the `XDocument` the C# used:
//! enough to find an element by namespace and local name, change its text, add a child with the
//! right prefix, drop what should go, and write the rest back as it was.
//!
//! **Untrusted input.** A document type declaration is never processed — the parse fails on one —
//! and only the five predefined entities and character references are resolved, so a packet from
//! the internet cannot make this fetch a URL or expand a billion entities.

use quick_xml::Reader;
use quick_xml::events::Event;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Node {
    Element(Element),
    Text(String),
    CData(String),
    Comment(String),
    /// Target and content, as written between `<?` and `?>`.
    Pi(String),
    /// The attributes of the XML declaration, as written.
    Decl(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Element {
    /// The qualified name as written: `rdf:Description`.
    pub name: String,
    /// Qualified name and unescaped value, in order.
    pub attributes: Vec<(String, String)>,
    pub children: Vec<Node>,
    /// Written as `<x/>`, and written back that way while it stays empty.
    pub empty: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Document {
    pub nodes: Vec<Node>,
}

/// The namespace declarations in force at an element, innermost last.
#[derive(Clone, Debug, Default)]
pub(crate) struct Scope {
    bindings: Vec<(String, String)>,
}

impl Scope {
    pub fn root() -> Scope {
        Scope {
            bindings: vec![(
                "xml".to_string(),
                "http://www.w3.org/XML/1998/namespace".to_string(),
            )],
        }
    }

    /// This scope with `element`'s own declarations added: the scope its name and attributes
    /// are read in.
    pub fn child(&self, element: &Element) -> Scope {
        let mut bindings = self.bindings.clone();
        bindings.extend(element.declarations());
        Scope { bindings }
    }

    fn uri_of(&self, prefix: &str) -> Option<&str> {
        self.bindings
            .iter()
            .rev()
            .find(|(p, _)| p == prefix)
            .map(|(_, uri)| uri.as_str())
    }

    /// An element's namespace and local name. An unprefixed element takes the default namespace.
    pub fn resolve_element<'a>(&self, qualified: &'a str) -> (String, &'a str) {
        match qualified.split_once(':') {
            Some((prefix, local)) => (self.uri_of(prefix).unwrap_or("").to_string(), local),
            None => (self.uri_of("").unwrap_or("").to_string(), qualified),
        }
    }

    /// An attribute's namespace and local name. An unprefixed attribute is in no namespace.
    pub fn resolve_attribute<'a>(&self, qualified: &'a str) -> (String, &'a str) {
        match qualified.split_once(':') {
            Some((prefix, local)) => (self.uri_of(prefix).unwrap_or("").to_string(), local),
            None => (String::new(), qualified),
        }
    }

    /// The prefix in force for a namespace: `""` for the default namespace, None when nothing
    /// declares it.
    pub fn prefix_for(&self, namespace: &str) -> Option<&str> {
        self.bindings
            .iter()
            .rev()
            .find(|(_, uri)| uri == namespace)
            .map(|(p, _)| p.as_str())
    }

    /// The qualified name for an element in `namespace`, or None when no prefix is in force.
    pub fn qualify(&self, namespace: &str, local: &str) -> Option<String> {
        let prefix = self.prefix_for(namespace)?;
        Some(if prefix.is_empty() {
            local.to_string()
        } else {
            format!("{prefix}:{local}")
        })
    }
}

impl Element {
    pub fn new(name: &str) -> Element {
        Element {
            name: name.to_string(),
            attributes: Vec::new(),
            children: Vec::new(),
            empty: false,
        }
    }

    /// A new element holding one text node.
    pub fn with_text(name: &str, text: &str) -> Element {
        let mut e = Element::new(name);
        e.children.push(Node::Text(text.to_string()));
        e
    }

    pub fn declarations(&self) -> Vec<(String, String)> {
        self.attributes
            .iter()
            .filter_map(|(k, v)| {
                if k == "xmlns" {
                    Some((String::new(), v.clone()))
                } else {
                    k.strip_prefix("xmlns:").map(|p| (p.to_string(), v.clone()))
                }
            })
            .collect()
    }

    /// Whether this element, read in its own `scope`, is `namespace`:`local`.
    pub fn is(&self, scope: &Scope, namespace: &str, local: &str) -> bool {
        let (ns, name) = scope.resolve_element(&self.name);
        ns == namespace && name == local
    }

    /// All text under the element, as `XElement.Value`.
    pub fn text(&self) -> String {
        let mut out = String::new();
        fn gather(nodes: &[Node], out: &mut String) {
            for node in nodes {
                match node {
                    Node::Text(t) | Node::CData(t) => out.push_str(t),
                    Node::Element(e) => gather(&e.children, out),
                    _ => {}
                }
            }
        }
        gather(&self.children, &mut out);
        out
    }

    /// Replaces everything under the element with one text node.
    pub fn set_text(&mut self, value: &str) {
        self.children = vec![Node::Text(value.to_string())];
        self.empty = false;
    }

    pub fn child_elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            _ => None,
        })
    }

    /// Indices into `children` of the element children that are `namespace`:`local`, where
    /// `scope` is this element's own scope.
    pub fn find_children(&self, scope: &Scope, namespace: &str, local: &str) -> Vec<usize> {
        self.children
            .iter()
            .enumerate()
            .filter(|(_, n)| match n {
                Node::Element(e) => e.is(&scope.child(e), namespace, local),
                _ => false,
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub fn first_child(&self, scope: &Scope, namespace: &str, local: &str) -> Option<&Element> {
        let index = *self.find_children(scope, namespace, local).first()?;
        match &self.children[index] {
            Node::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn child_mut(&mut self, index: usize) -> &mut Element {
        match &mut self.children[index] {
            Node::Element(e) => e,
            _ => panic!("not an element child"),
        }
    }

    /// Removes the element children that are `namespace`:`local`; true when there were any.
    pub fn remove_children(&mut self, scope: &Scope, namespace: &str, local: &str) -> bool {
        let before = self.children.len();
        self.children.retain(|n| match n {
            Node::Element(e) => !e.is(&scope.child(e), namespace, local),
            _ => true,
        });
        before != self.children.len()
    }

    /// The value of the attribute `namespace`:`local`, where `scope` is this element's own
    /// scope.
    pub fn attribute(&self, scope: &Scope, namespace: &str, local: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(k, _)| {
                let (ns, name) = scope.resolve_attribute(k);
                ns == namespace && name == local
            })
            .map(|(_, v)| v.as_str())
    }

    /// Removes the attributes that are `namespace`:`local`; true when there were any.
    pub fn remove_attributes(&mut self, scope: &Scope, namespace: &str, local: &str) -> bool {
        let before = self.attributes.len();
        self.attributes.retain(|(k, _)| {
            let (ns, name) = scope.resolve_attribute(k);
            !(ns == namespace && name == local)
        });
        before != self.attributes.len()
    }

    /// Declares `prefix` for `namespace` on this element, unless the scope already has one.
    pub fn declare(&mut self, scope: &Scope, prefix: &str, namespace: &str) {
        if scope.prefix_for(namespace).is_none() {
            self.attributes
                .push((format!("xmlns:{prefix}"), namespace.to_string()));
        }
    }

    /// The qualified name a new child in `namespace` should carry here. When nothing in scope
    /// declares it, the child is given a default-namespace declaration of its own.
    pub fn name_for_child(&self, scope: &Scope, namespace: &str, local: &str) -> (String, bool) {
        let scope = scope.child(self);
        match scope.qualify(namespace, local) {
            Some(name) => (name, false),
            None => (local.to_string(), true),
        }
    }

    /// Appends a child element in `namespace`, declaring the namespace on it if nothing else does.
    pub fn append_child(
        &mut self,
        scope: &Scope,
        namespace: &str,
        local: &str,
        mut child: Element,
    ) -> &mut Element {
        let (name, declare) = self.name_for_child(scope, namespace, local);
        child.name = name;
        if declare && !namespace.is_empty() {
            child
                .attributes
                .insert(0, ("xmlns".to_string(), namespace.to_string()));
        }
        self.children.push(Node::Element(child));
        self.empty = false;
        self.child_mut(self.children.len() - 1)
    }
}

impl Document {
    /// The tree, or None when the text is not well-formed XML, declares a document type, or
    /// uses an entity nobody defined.
    pub fn parse(text: &str) -> Option<Document> {
        let mut reader = Reader::from_str(text);
        let mut stack: Vec<Element> = Vec::new();
        let mut document = Document::default();

        fn push(stack: &mut [Element], document: &mut Document, node: Node) {
            match stack.last_mut() {
                Some(parent) => parent.children.push(node),
                None => document.nodes.push(node),
            }
        }

        loop {
            match reader.read_event().ok()? {
                Event::Start(start) => {
                    let mut element = Element::new(start.name().as_ref());
                    for attribute in start.attributes() {
                        let attribute = attribute.ok()?;
                        let value = attribute
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .ok()?
                            .into_owned();
                        element
                            .attributes
                            .push((attribute.key.as_ref().to_string(), value));
                    }
                    stack.push(element);
                }
                Event::Empty(start) => {
                    let mut element = Element::new(start.name().as_ref());
                    element.empty = true;
                    for attribute in start.attributes() {
                        let attribute = attribute.ok()?;
                        let value = attribute
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .ok()?
                            .into_owned();
                        element
                            .attributes
                            .push((attribute.key.as_ref().to_string(), value));
                    }
                    push(&mut stack, &mut document, Node::Element(element));
                }
                Event::End(_) => {
                    let element = stack.pop()?;
                    push(&mut stack, &mut document, Node::Element(element));
                }
                Event::Text(text) => {
                    let text = text.into_inner().into_owned();
                    append_text(&mut stack, &mut document, &text);
                }
                Event::GeneralRef(reference) => {
                    let resolved = resolve_reference(&reference.into_inner())?;
                    append_text(&mut stack, &mut document, &resolved);
                }
                Event::CData(data) => {
                    let data = data.into_inner().into_owned();
                    push(&mut stack, &mut document, Node::CData(data));
                }
                Event::Comment(comment) => {
                    let comment = comment.into_inner().into_owned();
                    push(&mut stack, &mut document, Node::Comment(comment));
                }
                Event::PI(pi) => {
                    let pi = pi.into_inner().into_owned();
                    push(&mut stack, &mut document, Node::Pi(pi));
                }
                Event::Decl(decl) => {
                    let raw = (*decl).trim().to_string();
                    push(&mut stack, &mut document, Node::Decl(raw));
                }
                Event::DocType(_) => return None,
                Event::Eof => break,
            }
        }

        if !stack.is_empty() || !document.nodes.iter().any(|n| matches!(n, Node::Element(_))) {
            return None;
        }
        Some(document)
    }

    pub fn root(&self) -> Option<&Element> {
        self.nodes.iter().find_map(|n| match n {
            Node::Element(e) => Some(e),
            _ => None,
        })
    }

    pub fn root_mut(&mut self) -> Option<&mut Element> {
        self.nodes.iter_mut().find_map(|n| match n {
            Node::Element(e) => Some(e),
            _ => None,
        })
    }

    /// Every element in document order with its own scope.
    pub fn visit(&self, mut f: impl FnMut(&Element, &Scope)) {
        fn walk(nodes: &[Node], scope: &Scope, f: &mut impl FnMut(&Element, &Scope)) {
            for node in nodes {
                if let Node::Element(e) = node {
                    let own = scope.child(e);
                    f(e, &own);
                    walk(&e.children, &own, f);
                }
            }
        }
        walk(&self.nodes, &Scope::root(), &mut f);
    }

    /// Every element in document order with its own scope, for changing.
    pub fn visit_mut(&mut self, mut f: impl FnMut(&mut Element, &Scope)) {
        fn walk(nodes: &mut [Node], scope: &Scope, f: &mut impl FnMut(&mut Element, &Scope)) {
            for node in nodes {
                if let Node::Element(e) = node {
                    let own = scope.child(e);
                    f(e, &own);
                    walk(&mut e.children, &own, f);
                }
            }
        }
        walk(&mut self.nodes, &Scope::root(), &mut f);
    }

    /// The document written back out. `declaration` false omits the XML declaration.
    pub fn serialize(&self, declaration: bool) -> String {
        let mut out = String::new();
        for node in &self.nodes {
            if !declaration && matches!(node, Node::Decl(_)) {
                continue;
            }
            write_node(node, &mut out);
        }
        out
    }
}

fn append_text(stack: &mut [Element], document: &mut Document, text: &str) {
    let children = match stack.last_mut() {
        Some(parent) => &mut parent.children,
        None => &mut document.nodes,
    };
    if let Some(Node::Text(last)) = children.last_mut() {
        last.push_str(text);
    } else {
        children.push(Node::Text(text.to_string()));
    }
}

/// The five entities XML defines and character references; None for anything else, since
/// nothing here reads a DTD that could define more.
fn resolve_reference(name: &str) -> Option<String> {
    match name {
        "amp" => Some("&".to_string()),
        "lt" => Some("<".to_string()),
        "gt" => Some(">".to_string()),
        "quot" => Some("\"".to_string()),
        "apos" => Some("'".to_string()),
        _ => {
            let code = name.strip_prefix('#')?;
            let value = match code.strip_prefix('x') {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => code.parse::<u32>().ok()?,
            };
            Some(char::from_u32(value)?.to_string())
        }
    }
}

pub(crate) fn write_element(element: &Element, out: &mut String) {
    out.push('<');
    out.push_str(&element.name);
    for (key, value) in &element.attributes {
        out.push(' ');
        out.push_str(key);
        out.push_str("=\"");
        escape_attribute(value, out);
        out.push('"');
    }
    if element.children.is_empty() && element.empty {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for child in &element.children {
        write_node(child, out);
    }
    out.push_str("</");
    out.push_str(&element.name);
    out.push('>');
}

fn write_node(node: &Node, out: &mut String) {
    match node {
        Node::Element(e) => write_element(e, out),
        Node::Text(t) => escape_text(t, out),
        Node::CData(t) => {
            out.push_str("<![CDATA[");
            out.push_str(t);
            out.push_str("]]>");
        }
        Node::Comment(t) => {
            out.push_str("<!--");
            out.push_str(t);
            out.push_str("-->");
        }
        Node::Pi(t) => {
            out.push_str("<?");
            out.push_str(t);
            out.push_str("?>");
        }
        Node::Decl(t) => {
            out.push_str("<?xml ");
            out.push_str(t);
            out.push_str("?>");
        }
    }
}

fn escape_text(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

fn escape_attribute(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}
