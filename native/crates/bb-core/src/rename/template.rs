//! Parses a name template into segments, once, so the expander does no string scanning per item
//! and the dialog can report an unusable template while the name is still being typed.
//! Spec: Core/Services/Rename/RenameTemplate.cs.
//!
//! Token names are matched case-insensitively, the way `CommandTemplate` matches its own — one app
//! should not hold two conventions. `{{` and `}}` are literal braces, which is the only reason a
//! strict reading of a lone brace is affordable: '{' is a legal filename character, so a template
//! that wants one has to be able to say so.

use super::{MetadataFieldKind, MetadataFieldSpec, MetadataFields};

/// What one piece of a parsed template stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenamePart {
    /// Text to copy out as-is.
    Literal,
    /// The whole name after find/replace and the case transform.
    Name,
    /// The name without its extension, after find/replace and the case transform.
    Base,
    /// The extension, leading dot included; empty for a folder.
    Extension,
    /// The containing folder's name — never its path.
    Parent,
    /// The counter. The argument holds the padding digits.
    Counter,
    /// The last-modified date. The argument holds the format.
    Modified,
    /// A field of the file's own metadata — `{artist}`, `{track:00}`, `{taken:yyyy-MM}`. The
    /// argument holds the field's id, then a colon and its padding or date format when it has one.
    Tag,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenameSegment {
    pub part: RenamePart,
    /// The literal text, or a token's argument; empty when it has none.
    pub argument: String,
}

impl RenameSegment {
    fn new(part: RenamePart, argument: &str) -> RenameSegment {
        RenameSegment {
            part,
            argument: argument.to_string(),
        }
    }
}

pub struct RenameTemplate;

impl RenameTemplate {
    /// Parses `template`, or explains why it cannot be used.
    pub fn parse(template: &str) -> Result<Vec<RenameSegment>, String> {
        let chars: Vec<char> = template.chars().collect();
        let mut segments = Vec::new();
        let mut literal = String::new();

        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];

            if c == '}' {
                // Doubled is a literal brace; alone it is a closer with nothing to close.
                if chars.get(i + 1) == Some(&'}') {
                    literal.push('}');
                    i += 2;
                    continue;
                }
                return Err(
                    "A name template can't contain a lone '}' \u{2014} write '}}' for a literal brace."
                        .to_string(),
                );
            }

            if c != '{' {
                literal.push(c);
                i += 1;
                continue;
            }

            if chars.get(i + 1) == Some(&'{') {
                literal.push('{');
                i += 2;
                continue;
            }

            let Some(close) = chars[i + 1..].iter().position(|&c| c == '}') else {
                return Err(
                    "A name template can't contain a lone '{' \u{2014} write '{{' for a literal brace."
                        .to_string(),
                );
            };
            let close = i + 1 + close;

            if !literal.is_empty() {
                segments.push(RenameSegment::new(RenamePart::Literal, &literal));
                literal.clear();
            }

            let body: String = chars[i + 1..close].iter().collect();
            segments.push(Self::token(&body)?);
            i = close + 1;
        }

        if !literal.is_empty() {
            segments.push(RenameSegment::new(RenamePart::Literal, &literal));
        }
        Ok(segments)
    }

    /// True when `template` holds at least one token.
    pub fn has_token(template: &str) -> bool {
        Self::parse(template).is_ok_and(|s| s.iter().any(|s| s.part != RenamePart::Literal))
    }

    /// True when `template` names a metadata field, and so needs the sources' tags read before it
    /// can be applied.
    pub fn uses_tags(template: &str) -> bool {
        Self::parse(template).is_ok_and(|s| s.iter().any(|s| s.part == RenamePart::Tag))
    }

    /// The field and argument a `Tag` segment carries.
    pub fn tag_of(segment: &RenameSegment) -> (&'static MetadataFieldSpec, &str) {
        let (id, argument) = match segment.argument.split_once(':') {
            Some((id, argument)) => (id, argument),
            None => (segment.argument.as_str(), ""),
        };
        (
            MetadataFields::find(id).expect("a Tag segment names a field"),
            argument,
        )
    }

    /// True when the parsed template uses any of `parts`.
    pub fn uses(segments: &[RenameSegment], parts: &[RenamePart]) -> bool {
        segments.iter().any(|s| parts.contains(&s.part))
    }

    fn token(body: &str) -> Result<RenameSegment, String> {
        let (name, argument) = match body.split_once(':') {
            Some((name, argument)) => (name.trim(), argument),
            None => (body.trim(), ""),
        };

        let part = match name.to_lowercase().as_str() {
            "name" => Some(RenamePart::Name),
            "base" => Some(RenamePart::Base),
            "ext" => Some(RenamePart::Extension),
            "parent" => Some(RenamePart::Parent),
            "n" => Some(RenamePart::Counter),
            "modified" => Some(RenamePart::Modified),
            _ => None,
        };

        if part.is_none()
            && let Some(field) = MetadataFields::find(name)
        {
            match field.kind {
                MetadataFieldKind::Number
                    if !argument.is_empty() && !argument.chars().all(|c| c == '0') =>
                {
                    return Err(format!(
                        "'{{{body}}}' is not a width \u{2014} write {{{}:00}} to pad to two digits.",
                        field.id
                    ));
                }
                MetadataFieldKind::Number | MetadataFieldKind::Date => {}
                _ => {
                    if !argument.is_empty() {
                        return Err(format!("'{{{}}}' does not take a ':' argument.", field.id));
                    }
                }
            }
            let argument = if argument.is_empty() {
                field.id.to_string()
            } else {
                format!("{}:{argument}", field.id)
            };
            return Ok(RenameSegment::new(RenamePart::Tag, &argument));
        }

        let Some(part) = part else {
            return Err(format!(
                "'{{{body}}}' is not a name template token. Try {{name}}, {{base}}, {{ext}}, \
                 {{parent}}, {{n}}, {{modified}}, or a tag such as {{artist}} or {{taken}} \
                 \u{2014} or write '{{{{' for a literal brace."
            ));
        };

        match part {
            RenamePart::Counter
                if !argument.is_empty() && !argument.bytes().all(|b| b.is_ascii_digit()) =>
            {
                return Err(format!(
                    "'{{{body}}}' is not a counter width \u{2014} write {{n:000}} to pad to three digits."
                ));
            }
            RenamePart::Modified | RenamePart::Counter => {}
            _ => {
                if !argument.is_empty() {
                    return Err(format!("'{{{name}}}' does not take a ':' argument."));
                }
            }
        }

        Ok(RenameSegment::new(part, argument))
    }
}
