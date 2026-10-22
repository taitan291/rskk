use std::{collections::HashSet, fs, path::Path};

use skk_core::{Candidate, Dictionary};
use tracing::warn;

use crate::{DictionaryIndex, UserDictionary, parse_dictionary};

/// User-first dictionary stack with ordered system dictionaries.
pub struct LayeredDictionary {
    user: UserDictionary,
    systems: Vec<DictionaryIndex>,
    suppressed: HashSet<(String, Option<char>, String)>,
}

impl LayeredDictionary {
    pub fn open(
        system_paths: &[impl AsRef<Path>],
        user_path: impl AsRef<Path>,
        immediately_rw: bool,
    ) -> std::io::Result<Self> {
        let mut systems = Vec::new();
        for path in system_paths {
            let path = path.as_ref();
            let bytes = match fs::read(path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    warn!(path = %path.display(), %error, "could not read system dictionary; skipping it");
                    continue;
                }
            };
            match parse_dictionary(&bytes, None) {
                Ok(report) => systems.push(report.index),
                Err(error) => {
                    warn!(path = %path.display(), %error, "failed to load system dictionary")
                }
            }
        }
        let user = UserDictionary::open(user_path, immediately_rw)?;
        Ok(Self {
            user,
            systems,
            suppressed: HashSet::new(),
        })
    }

    pub fn empty(user_path: impl AsRef<Path>, immediately_rw: bool) -> std::io::Result<Self> {
        Ok(Self {
            user: UserDictionary::open(user_path, immediately_rw)?,
            systems: Vec::new(),
            suppressed: HashSet::new(),
        })
    }

    pub fn flush(&self) {
        self.user.flush();
    }
}

impl Dictionary for LayeredDictionary {
    fn lookup(&self, key: &str, okuri: Option<char>) -> Vec<Candidate> {
        let mut result = self.user.lookup(key, okuri);
        let mut seen: HashSet<String> = result
            .iter()
            .map(|candidate| candidate.text.clone())
            .collect();
        for system in &self.systems {
            for candidate in system.lookup(key, okuri) {
                if !self
                    .suppressed
                    .contains(&(key.to_owned(), okuri, candidate.text.clone()))
                    && seen.insert(candidate.text.clone())
                {
                    result.push(candidate);
                }
            }
        }
        result
    }

    fn complete(&self, prefix: &str) -> Vec<String> {
        self.user.complete(prefix)
    }

    fn learn(&mut self, key: &str, okuri: Option<char>, candidate: &Candidate) {
        self.suppressed
            .remove(&(key.to_owned(), okuri, candidate.text.clone()));
        self.user.learn(key, okuri, candidate);
    }

    fn purge(&mut self, key: &str, okuri: Option<char>, candidate: &Candidate) {
        self.user.purge(key, okuri, candidate);
        self.suppressed
            .insert((key.to_owned(), okuri, candidate.text.clone()));
    }
}

impl LayeredDictionary {
    pub fn add_system_index(&mut self, index: DictionaryIndex) {
        self.systems.push(index);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use tempfile::tempdir;

    #[test]
    fn looks_up_user_before_ordered_systems_and_deduplicates() {
        let dir = tempdir().unwrap();
        let user = dir.path().join("user-dict");
        let first = dir.path().join("first-dict");
        let second = dir.path().join("second-dict");
        fs::write(
            &user,
            ";; coding: utf-8\n;; okuri-nasi entries.\nkey /user/shared/\n",
        )
        .unwrap();
        fs::write(&first, ";; coding: utf-8\nkey /first/shared/\n").unwrap();
        fs::write(&second, ";; coding: utf-8\nkey /second/shared/\n").unwrap();

        let dictionary = LayeredDictionary::open(&[first, second], user, false).unwrap();
        let candidates = dictionary.lookup("key", None);
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.text.as_str())
                .collect::<Vec<_>>(),
            ["user", "shared", "first", "second"]
        );
    }
}
