//! Turns an authored `ThemeDefinition` into a complete `ResolvedTheme`. Resolution never fails:
//! theme files are hand-editable text and a base theme can legitimately be missing (a file still
//! syncing, a theme deleted while selected), so every problem becomes a `ThemeIssue` and the
//! caller still gets a usable theme. Colours are layered lowest to highest: the Dark+ root, then
//! the base chain from its far end inwards, then the theme's own colours, then the user's
//! per-token overrides. Spec: src/BertBrowser.Core/Theming/ThemeResolver.cs, ResolvedTheme.cs.

use std::ops::Index;

use super::{
    ColorMap, ThemeCatalog, ThemeColor, ThemeDefinition, ThemeIssue, ThemeIssueSeverity,
    ThemeToken, ids_equal,
};

/// A theme with every token in `ThemeToken::ALL` resolved to a concrete colour. This is what the
/// UI consumes; by construction a lookup can never miss, and the palette is indexed by the token's
/// discriminant.
#[derive(Clone, Debug)]
pub struct ResolvedTheme {
    pub id: String,
    pub name: String,
    pub is_dark: bool,
    /// Problems found while resolving. Empty for a well-formed theme.
    pub issues: Vec<ThemeIssue>,
    colors: [ThemeColor; ThemeToken::COUNT],
}

impl ResolvedTheme {
    pub fn color(&self, token: ThemeToken) -> ThemeColor {
        self.colors[token.index()]
    }

    /// The colour for a token named by its key, or `None` for a key this version does not know.
    pub fn get(&self, key: &str) -> Option<ThemeColor> {
        ThemeToken::from_name(key).map(|t| self.color(t))
    }

    /// The whole palette, in `ThemeToken::ALL` order.
    pub fn palette(&self) -> &[ThemeColor; ThemeToken::COUNT] {
        &self.colors
    }

    pub fn colors(&self) -> impl Iterator<Item = (ThemeToken, ThemeColor)> + '_ {
        ThemeToken::ALL.iter().map(|&t| (t, self.color(t)))
    }
}

impl Index<ThemeToken> for ResolvedTheme {
    type Output = ThemeColor;

    fn index(&self, token: ThemeToken) -> &ThemeColor {
        &self.colors[token.index()]
    }
}

/// The lookup to pass when there is nothing to inherit from but the built-in floor.
pub fn no_lookup(_: &str) -> Option<&'static ThemeDefinition> {
    None
}

pub struct ThemeResolver;

/// Stands in for a token whose literal could not be parsed: deliberately unmissable.
const UNPARSEABLE: ThemeColor = ThemeColor::from_rgb(0xFF, 0x00, 0xFF);

impl ThemeResolver {
    /// `lookup` resolves a base id to its definition, user themes included; `ThemeCatalog::find`
    /// for the built-ins alone, `no_lookup` for none.
    pub fn resolve<'l>(
        definition: &ThemeDefinition,
        lookup: impl Fn(&str) -> Option<&'l ThemeDefinition>,
    ) -> ResolvedTheme {
        ThemeResolver::resolve_with(definition, lookup, None)
    }

    /// As `resolve`, with the user's per-token overrides layered on top.
    pub fn resolve_with<'l>(
        definition: &ThemeDefinition,
        lookup: impl Fn(&str) -> Option<&'l ThemeDefinition>,
        overrides: Option<&ColorMap>,
    ) -> ResolvedTheme {
        let mut issues = Vec::new();

        // Seed from the Dark+ root so the result is complete whatever the theme leaves out; that
        // is what lets the index be total.
        let mut colors = [UNPARSEABLE; ThemeToken::COUNT];
        for (key, literal) in ThemeCatalog::dark_plus().colors.iter() {
            if let Some(token) = ThemeToken::from_name(key) {
                colors[token.index()] = ThemeColor::try_parse(literal).unwrap_or(UNPARSEABLE);
            }
        }

        // Applied far end first, so a theme's own colours win over the base it inherits from.
        let chain = build_chain(definition, lookup, &mut issues);
        for layer in chain.iter().rev() {
            apply_layer(
                &layer.colors,
                &mut colors,
                &mut issues,
                &format!("theme '{}'", layer.id),
            );
        }

        if let Some(overrides) = overrides
            && !overrides.is_empty()
        {
            apply_layer(overrides, &mut colors, &mut issues, "your customisations");
        }

        ResolvedTheme {
            id: definition.id.clone(),
            name: definition.name.clone(),
            is_dark: definition.is_dark,
            issues,
            colors,
        }
    }

    /// Drops overrides that name an unknown token or that just restate the theme's own value, so
    /// the settings file does not accumulate dead entries as themes change between versions.
    pub fn prune_overrides(
        overrides: &ColorMap,
        theme_without_overrides: &ResolvedTheme,
    ) -> ColorMap {
        let mut pruned = ColorMap::new();
        for (key, literal) in overrides.iter() {
            let Some(token) = ThemeToken::from_name(key) else {
                continue;
            };
            let Some(color) = ThemeColor::try_parse(literal) else {
                continue;
            };
            if theme_without_overrides[token] == color {
                continue;
            }
            pruned.insert(key, &color.to_hex());
        }
        pruned
    }
}

/// The definition and everything it inherits from. The lookup's themes may outlive the definition
/// (the catalogue's are `'static`), so the chain is borrowed for the shorter of the two.
fn build_chain<'c, 'l: 'c>(
    definition: &'c ThemeDefinition,
    lookup: impl Fn(&str) -> Option<&'l ThemeDefinition>,
    issues: &mut Vec<ThemeIssue>,
) -> Vec<&'c ThemeDefinition> {
    let mut chain = vec![definition];
    let mut visited: Vec<&str> = vec![&definition.id];

    let mut current = definition;
    while let Some(base_id) = current.base_theme_id.as_deref()
        && !base_id.is_empty()
    {
        if visited.iter().any(|v| ids_equal(v, base_id)) {
            issues.push(ThemeIssue::new(
                ThemeIssueSeverity::Error,
                None,
                format!(
                    "Theme '{}' inherits from itself via '{base_id}'; the loop was ignored.",
                    definition.id
                ),
            ));
            break;
        }
        visited.push(base_id);

        let Some(parent) = lookup(base_id) else {
            issues.push(ThemeIssue::new(
                ThemeIssueSeverity::Warning,
                None,
                format!(
                    "Base theme '{base_id}' was not found; using {} for the colours '{}' does not set.",
                    ThemeCatalog::dark_plus().name,
                    current.id
                ),
            ));
            break;
        };

        chain.push(parent);
        current = parent;
    }

    chain
}

fn apply_layer(
    layer: &ColorMap,
    colors: &mut [ThemeColor; ThemeToken::COUNT],
    issues: &mut Vec<ThemeIssue>,
    source: &str,
) {
    for (key, literal) in layer.iter() {
        let Some(token) = ThemeToken::from_name(key) else {
            issues.push(ThemeIssue::new(
                ThemeIssueSeverity::Warning,
                Some(key),
                format!(
                    "'{key}' in {source} is not a colour this version knows about; it was ignored."
                ),
            ));
            continue;
        };

        let Some(color) = ThemeColor::try_parse(literal) else {
            issues.push(ThemeIssue::new(
                ThemeIssueSeverity::Warning,
                Some(key),
                format!(
                    "'{literal}' in {source} is not a colour; '{key}' kept its inherited value."
                ),
            ));
            continue;
        };

        colors[token.index()] = color;
    }
}
