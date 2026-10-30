//! Themes: the token list, ARGB colours and WCAG contrast, the built-in catalogue, the on-disk JSON
//! shape of a user theme, resolution of a sparse definition into a complete palette, and the pure
//! rules for "match Windows". Nothing here touches a window; the UI turns a `ResolvedTheme` into
//! brushes at the last moment. Spec: src/BertBrowser.Core/Theming/*.cs.

mod catalog;
mod color;
mod contrast;
mod definition;
mod id;
mod issue;
mod json;
mod resolver;
mod system;
mod token;

pub use catalog::ThemeCatalog;
pub use color::ThemeColor;
pub use contrast::ThemeContrast;
pub use definition::{ColorMap, ThemeDefinition};
pub use id::ThemeId;
pub use issue::{ThemeIssue, ThemeIssueSeverity};
pub use json::ThemeJson;
pub use resolver::{ResolvedTheme, ThemeResolver, no_lookup};
pub use system::{SystemAppearance, SystemThemeChoice, SystemThemeRules};
pub use token::{ThemeToken, ThemeTokenDescriptor};

/// Theme ids compare case-insensitively everywhere (the C# used `OrdinalIgnoreCase`): a slot in
/// settings spelled `NoRd` still finds Nord.
pub fn ids_equal(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_uppercase)
        .eq(b.chars().flat_map(char::to_uppercase))
}
