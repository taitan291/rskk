use std::collections::HashMap;

use thiserror::Error;

/// A replaceable, data-driven romaji table.
#[derive(Debug, Clone)]
pub struct KanaTable {
    entries: HashMap<String, String>,
}

#[derive(Debug, Error)]
pub enum KanaTableError {
    #[error("invalid kana table TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("kana table must contain a [kana] table")]
    MissingKana,
    #[error("kana table key must be non-empty ASCII")]
    InvalidKey,
}

impl KanaTable {
    pub fn standard() -> Self {
        Self::from_toml(include_str!("kana_table.toml")).expect("bundled kana table is valid")
    }

    pub fn from_toml(src: &str) -> Result<Self, KanaTableError> {
        Self::from_named_toml(src, "rom")
    }

    pub fn from_named_toml(src: &str, name: &str) -> Result<Self, KanaTableError> {
        let value: toml::Value = toml::from_str(src)?;
        let table = value
            .get("tables")
            .and_then(|tables| tables.get(name))
            .and_then(|table| table.get("kana"))
            .or_else(|| value.get(name))
            .or_else(|| value.get("kana"))
            .and_then(toml::Value::as_table)
            .ok_or(KanaTableError::MissingKana)?;
        let mut entries = HashMap::with_capacity(table.len());
        for (key, value) in table {
            if key.is_empty() || !key.is_ascii() {
                return Err(KanaTableError::InvalidKey);
            }
            let Some(value) = value.as_str() else {
                continue;
            };
            entries.insert(key.to_ascii_lowercase(), value.to_owned());
        }
        Ok(Self { entries })
    }

    pub fn overlay_toml(&mut self, src: &str, name: &str) -> Result<(), KanaTableError> {
        let table = Self::from_named_toml(src, name)?;
        self.entries.extend(table.entries);
        Ok(())
    }

    pub fn has_prefix(&self, prefix: &str) -> bool {
        self.entries.keys().any(|key| key.starts_with(prefix))
    }

    pub fn has_longer_prefix(&self, prefix: &str) -> bool {
        self.entries
            .keys()
            .any(|key| key.len() > prefix.len() && key.starts_with(prefix))
    }

    pub fn exact(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    pub fn longest_prefix(&self, input: &str) -> Option<(&str, &str)> {
        self.entries
            .iter()
            .filter(|(key, _)| input.starts_with(key.as_str()))
            .max_by_key(|(key, _)| key.len())
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }
}

impl Default for KanaTable {
    fn default() -> Self {
        Self::standard()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_custom_table() {
        let table =
            KanaTable::from_toml("[kana]\na = 'あ'\nai = 'あい'\nka = 'か'\nk = ''\n").unwrap();
        assert_eq!(table.exact("ka"), Some("か"));
        assert!(table.has_prefix("k"));
        assert!(table.has_longer_prefix("a"));
    }
}
