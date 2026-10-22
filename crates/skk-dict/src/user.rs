use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{self, Sender},
    },
    thread::{self, JoinHandle},
    time::SystemTime,
};

use skk_core::{Candidate, Dictionary};
use tempfile::NamedTempFile;
use tracing::warn;

use crate::{DictionaryIndex, Encoding, ParseError, parse_dictionary, parser::format_entry};

enum PersistMessage {
    Save(DictionaryIndex),
    Flush(Sender<()>),
}

/// Mutable UTF-8 user dictionary with a background atomic writer.
pub struct UserDictionary {
    path: PathBuf,
    index: Arc<Mutex<DictionaryIndex>>,
    persist: Option<Sender<PersistMessage>>,
    writer: Option<JoinHandle<()>>,
}

impl UserDictionary {
    pub fn open(path: impl AsRef<Path>, immediately_rw: bool) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let index = if path.exists() {
            match fs::read(&path).and_then(|bytes| {
                parse_user_dictionary(&bytes)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
            }) {
                Ok(report) => report.index,
                Err(error) => {
                    warn!(path = %path.display(), %error, "could not read user dictionary; starting empty");
                    DictionaryIndex::default()
                }
            }
        } else {
            DictionaryIndex::default()
        };
        let index = Arc::new(Mutex::new(index));
        let (persist, writer) = if immediately_rw {
            let (tx, rx) = mpsc::channel();
            let writer_path = path.clone();
            let handle = thread::Builder::new()
                .name("rskk-dictionary-writer".into())
                .spawn(move || writer_loop(writer_path, rx))?;
            (Some(tx), Some(handle))
        } else {
            (None, None)
        };
        Ok(Self {
            path,
            index,
            persist,
            writer,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn flush(&self) {
        let Some(tx) = &self.persist else { return };
        let (done_tx, done_rx) = mpsc::channel();
        if tx.send(PersistMessage::Flush(done_tx)).is_ok() {
            let _ = done_rx.recv();
        }
    }

    fn queue_snapshot(&self) {
        let Some(tx) = &self.persist else { return };
        if let Ok(index) = self.index.lock()
            && tx.send(PersistMessage::Save(index.clone())).is_err()
        {
            warn!(path = %self.path.display(), "dictionary writer stopped before saving");
        }
    }
}

impl Drop for UserDictionary {
    fn drop(&mut self) {
        self.persist.take();
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

impl Dictionary for UserDictionary {
    fn lookup(&self, key: &str, okuri: Option<char>) -> Vec<Candidate> {
        self.index
            .lock()
            .map(|index| index.lookup(key, okuri))
            .unwrap_or_default()
    }

    fn complete(&self, prefix: &str) -> Vec<String> {
        self.index
            .lock()
            .map(|index| index.complete(prefix))
            .unwrap_or_default()
    }

    fn learn(&mut self, key: &str, okuri: Option<char>, candidate: &Candidate) {
        if let Ok(mut index) = self.index.lock() {
            let map = if okuri.is_some() {
                &mut index.okuri_ari
            } else {
                &mut index.okuri_nasi
            };
            let values = map.entry(key.to_owned()).or_default();
            values.retain(|item| item.text != candidate.text);
            values.insert(0, candidate.clone());
        }
        self.queue_snapshot();
    }

    fn purge(&mut self, key: &str, okuri: Option<char>, candidate: &Candidate) {
        if let Ok(mut index) = self.index.lock() {
            let map = if okuri.is_some() {
                &mut index.okuri_ari
            } else {
                &mut index.okuri_nasi
            };
            if let Some(values) = map.get_mut(key) {
                values.retain(|item| item.text != candidate.text);
            }
        }
        self.queue_snapshot();
    }
}

fn writer_loop(path: PathBuf, rx: mpsc::Receiver<PersistMessage>) {
    let mut known_mtime = file_mtime(&path);
    for message in rx {
        match message {
            PersistMessage::Save(mut index) => {
                let current_mtime = file_mtime(&path);
                if current_mtime != known_mtime
                    && let Ok(bytes) = fs::read(&path)
                    && let Ok(external) = parse_user_dictionary(&bytes)
                {
                    merge_external_after_local(&mut index, external.index);
                }
                match write_user_dictionary_atomic(&path, &index) {
                    Ok(()) => known_mtime = file_mtime(&path),
                    Err(error) => {
                        warn!(path = %path.display(), %error, "could not persist user dictionary")
                    }
                }
            }
            PersistMessage::Flush(done) => {
                let _ = done.send(());
            }
        }
    }
}

fn parse_user_dictionary(bytes: &[u8]) -> Result<crate::ParseReport, ParseError> {
    if std::str::from_utf8(bytes).is_err() {
        return Err(ParseError::Undecodable);
    }
    if let Some(first_line) = bytes.split(|byte| *byte == b'\n').next() {
        let first_line = String::from_utf8_lossy(first_line).to_ascii_lowercase();
        if first_line.contains("coding:")
            && !first_line.contains("utf-8")
            && !first_line.contains("utf8")
        {
            return Err(ParseError::Undecodable);
        }
    }
    parse_dictionary(bytes, Some(Encoding::Utf8))
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn merge_external_after_local(local: &mut DictionaryIndex, external: DictionaryIndex) {
    for (key, values) in external.okuri_ari {
        merge_values(local.okuri_ari.entry(key).or_default(), values);
    }
    for (key, values) in external.okuri_nasi {
        merge_values(local.okuri_nasi.entry(key).or_default(), values);
    }
}

fn merge_values(target: &mut Vec<Candidate>, incoming: Vec<Candidate>) {
    for candidate in incoming {
        if !target.iter().any(|old| old.text == candidate.text) {
            target.push(candidate);
        }
    }
}

/// Write UTF-8 dictionary contents via a same-directory temp file and atomic rename.
pub fn write_user_dictionary_atomic(
    path: impl AsRef<Path>,
    index: &DictionaryIndex,
) -> io::Result<()> {
    let path = path.as_ref();
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temp = NamedTempFile::new_in(parent)?;
    write_index(temp.as_file_mut(), index)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn write_index(file: &mut File, index: &DictionaryIndex) -> io::Result<()> {
    writeln!(file, ";; -*- coding: utf-8 -*-")?;
    writeln!(file, ";; okuri-ari entries.")?;
    write_map(file, &index.okuri_ari)?;
    writeln!(file, ";; okuri-nasi entries.")?;
    write_map(file, &index.okuri_nasi)
}

fn write_map(file: &mut File, map: &HashMap<String, Vec<Candidate>>) -> io::Result<()> {
    let mut keys: Vec<_> = map.keys().collect();
    keys.sort();
    for key in keys {
        let values = &map[key];
        if !values.is_empty() {
            writeln!(file, "{}", format_entry(key, values))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::EUC_JP;
    use tempfile::tempdir;

    #[test]
    fn user_dictionary_requires_utf8() {
        let source = ";; -*- coding: euc-jp -*-\nよみ /語/\n";
        let (bytes, _, errors) = EUC_JP.encode(source);
        assert!(!errors);
        assert!(parse_user_dictionary(&bytes).is_err());
    }

    #[test]
    fn learns_moves_to_front_persists_and_reloads() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("user-dict");
        let mut dictionary = UserDictionary::open(&path, true).unwrap();
        dictionary.learn("かんじ", None, &Candidate::new("感じ"));
        dictionary.learn("かんじ", None, &Candidate::new("漢字"));
        dictionary.flush();
        drop(dictionary);
        let loaded = UserDictionary::open(&path, false).unwrap();
        assert_eq!(loaded.lookup("かんじ", None)[0].text, "漢字");
    }

    #[test]
    fn atomic_write_keeps_old_file_until_replace() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("user-dict");
        fs::write(&path, "old").unwrap();
        let mut index = DictionaryIndex::default();
        index
            .okuri_nasi
            .insert("よみ".into(), vec![Candidate::new("語/彙")]);
        write_user_dictionary_atomic(&path, &index).unwrap();
        let read = fs::read_to_string(path).unwrap();
        assert!(read.contains("語\\057彙"));
        assert!(
            parse_dictionary(read.as_bytes(), Some(Encoding::Utf8))
                .unwrap()
                .index
                .lookup("よみ", None)
                .iter()
                .any(|c| c.text == "語/彙")
        );
    }
}
