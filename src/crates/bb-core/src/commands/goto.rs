//! Recognises a path typed into the palette, so the palette can offer to go there: `C:\Windows`,
//! `~\Downloads`, `%TEMP%` and `\\server\share` are destinations rather than searches. Pure: nothing
//! here touches the disk or the environment; both are handed in. Spec: Core/Services/Commands/GoToRules.cs.

/// What a typed path came to, before anyone has looked at the disk. `folder` is what to list for
/// completions; `prefix` is the start of a name being typed inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoToTarget {
    pub path: String,
    pub folder: String,
    pub prefix: String,
}

impl GoToTarget {
    pub fn new(path: &str, folder: &str, prefix: &str) -> GoToTarget {
        GoToTarget {
            path: path.to_string(),
            folder: folder.to_string(),
            prefix: prefix.to_string(),
        }
    }
}

pub struct GoToRules;

impl GoToRules {
    /// The path this text names, or None when it is not one, in which case it is a search for a
    /// command. `variable` looks an environment variable up by name; `home` is what `~` stands for.
    pub fn parse(
        text: &str,
        variable: impl Fn(&str) -> Option<String>,
        home: &str,
    ) -> Option<GoToTarget> {
        let typed = text.trim().trim_matches('"');
        if typed.is_empty() {
            return None;
        }
        let mut expanded = expand(typed, &variable, home)?;
        if !looks_rooted(&expanded) {
            return None;
        }
        // A bare drive is its root: "c:" on its own means the top of C.
        if expanded.len() == 2 {
            expanded.push('\\');
        }
        let expanded = expanded.replace('/', "\\");
        if expanded.ends_with('\\') {
            return Some(GoToTarget::new(&expanded, &expanded, ""));
        }
        match expanded.rfind('\\') {
            None => Some(GoToTarget::new(&expanded, &expanded, "")),
            Some(cut) => Some(GoToTarget::new(
                &expanded,
                &expanded[..=cut],
                &expanded[cut + 1..],
            )),
        }
    }
}

/// Fills in a leading `~` and any `%NAME%`. None when a variable is named that does not exist, since
/// a path built on nothing leads nowhere; and for an unclosed `%`, which is still being typed.
fn expand(typed: &str, variable: &impl Fn(&str) -> Option<String>, home: &str) -> Option<String> {
    if typed == "~" {
        return Some(home.to_string());
    }
    let mut text = if typed.starts_with("~\\") || typed.starts_with("~/") {
        format!("{}{}", home.trim_end_matches('\\'), &typed[1..])
    } else {
        typed.to_string()
    };
    let mut at = 0;
    loop {
        let Some(open) = text[at..].find('%').map(|i| at + i) else {
            return Some(text);
        };
        let close = text[open + 1..].find('%').map(|i| open + 1 + i)?;
        let name = &text[open + 1..close];
        if name.is_empty() {
            return None;
        }
        let value = variable(name).filter(|v| !v.is_empty())?;
        text = format!("{}{}{}", &text[..open], value, &text[close + 1..]);
        at = open + value.len();
    }
}

/// A drive letter and a colon, or the two slashes of a share.
fn looks_rooted(path: &str) -> bool {
    let b = path.as_bytes();
    (b.len() >= 2
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b.len() == 2 || b[2] == b'\\' || b[2] == b'/'))
        || path.starts_with("\\\\")
}
