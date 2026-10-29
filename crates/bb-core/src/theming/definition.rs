//! A theme as authored: an identity plus a, possibly sparse, set of colour overrides. Anything it
//! leaves out comes from `base_theme_id`, so a user theme can be three lines long. This is also the
//! on-disk shape of a `*.json` file in the user's themes folder.
//! Spec: src/BertBrowser.Core/Theming/ThemeDefinition.cs.

use std::fmt;

use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Token key to colour literal, in authoring order. Insertion order is kept (a `BTreeMap` would
/// sort an exported file alphabetically and a `HashMap` would shuffle it), and a repeated key
/// replaces the earlier value in place, as a dictionary indexer does.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ColorMap(Vec<(String, String)>);

impl ColorMap {
    pub fn new() -> ColorMap {
        ColorMap(Vec::new())
    }

    pub fn insert(&mut self, key: &str, literal: &str) {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = literal.to_string(),
            None => self.0.push((key.to_string(), literal.to_string())),
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(k, _)| k.as_str())
    }
}

impl<K: AsRef<str>, V: AsRef<str>> FromIterator<(K, V)> for ColorMap {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> ColorMap {
        let mut map = ColorMap::new();
        for (k, v) in iter {
            map.insert(k.as_ref(), v.as_ref());
        }
        map
    }
}

impl<const N: usize> From<[(&str, &str); N]> for ColorMap {
    fn from(entries: [(&str, &str); N]) -> ColorMap {
        entries.into_iter().collect()
    }
}

impl Serialize for ColorMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for ColorMap {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<ColorMap, D::Error> {
        struct MapVisitor;

        impl<'de> Visitor<'de> for MapVisitor {
            type Value = ColorMap;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object of token keys to colour literals")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<ColorMap, A::Error> {
                let mut map = ColorMap::new();
                while let Some((k, v)) = access.next_entry::<String, String>()? {
                    map.insert(&k, &v);
                }
                Ok(map)
            }
        }

        deserializer.deserialize_map(MapVisitor)
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ThemeDefinition {
    pub id: String,
    pub name: String,
    /// Theme to inherit unset colours from. `None` for the two built-in roots.
    #[serde(rename = "base")]
    pub base_theme_id: Option<String>,
    /// Whether this reads as a dark theme. Metadata only: it selects the OS dark-mode window frame
    /// and how far hidden items are dimmed; it never affects colour resolution.
    pub is_dark: bool,
    /// Token key (see `ThemeToken`) to colour literal, e.g. `"#1E1E1E"`.
    pub colors: ColorMap,
    /// False for themes loaded from the user's themes folder, which can be edited and deleted.
    #[serde(skip)]
    pub is_built_in: bool,
}

impl ThemeDefinition {
    pub fn with_colors(&self, colors: ColorMap) -> ThemeDefinition {
        ThemeDefinition {
            colors,
            ..self.clone()
        }
    }
}
