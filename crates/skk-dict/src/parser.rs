use std::collections::HashMap;

use encoding_rs::{EUC_JP, UTF_8};
use skk_core::Candidate;
use thiserror::Error;
use tracing::warn;

use crate::escape::decode_concat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    EucJp,
}

#[derive(Debug, Clone, Default)]
pub struct DictionaryIndex {
    pub okuri_ari: HashMap<String, Vec<Candidate>>,
    pub okuri_nasi: HashMap<String, Vec<Candidate>>,
}

impl DictionaryIndex {
    pub fn lookup(&self, key: &str, okuri: Option<char>) -> Vec<Candidate> {
        let map = if okuri.is_some() {
            &self.okuri_ari
        } else {
            &self.okuri_nasi
        };
        map.get(key).cloned().unwrap_or_default()
    }

    /// Return longer okuri-nasi keys beginning with `prefix`, in stable order.
    pub fn complete(&self, prefix: &str) -> Vec<String> {
        let mut keys: Vec<_> = self
            .okuri_nasi
            .keys()
            .filter(|key| key.len() > prefix.len() && key.starts_with(prefix))
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    pub fn merge_ordered(&mut self, other: DictionaryIndex) {
        for (key, values) in other.okuri_ari {
            merge_candidates(self.okuri_ari.entry(key).or_default(), values);
        }
        for (key, values) in other.okuri_nasi {
            merge_candidates(self.okuri_nasi.entry(key).or_default(), values);
        }
    }
}

fn merge_candidates(target: &mut Vec<Candidate>, values: Vec<Candidate>) {
    for value in values {
        if !target.iter().any(|current| current.text == value.text) {
            target.push(value);
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParseReport {
    pub index: DictionaryIndex,
    pub encoding: Encoding,
    pub skipped_lines: usize,
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("could not decode dictionary as UTF-8 or EUC-JP")]
    Undecodable,
}

pub fn parse_dictionary(
    bytes: &[u8],
    configured: Option<Encoding>,
) -> Result<ParseReport, ParseError> {
    let encoding = coding_header(bytes).or(configured).unwrap_or_else(|| {
        if UTF_8.decode_without_bom_handling(bytes).1 {
            Encoding::EucJp
        } else {
            Encoding::Utf8
        }
    });
    let mut index = DictionaryIndex::default();
    let mut okuri_ari = false;
    let mut skipped_lines = 0;
    for (line_no, raw) in bytes.split(|byte| *byte == b'\n').enumerate() {
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        let (line, errors) = decode_line(raw, encoding);
        if errors {
            skipped_lines += 1;
            warn!(
                line = line_no + 1,
                "skipping invalid encoded SKK dictionary line"
            );
            continue;
        }
        let line = line.trim_end();
        if line.contains("okuri-ari entries") {
            okuri_ari = true;
            continue;
        }
        if line.contains("okuri-nasi entries") {
            okuri_ari = false;
            continue;
        }
        if line.starts_with(';') || line.is_empty() {
            continue;
        }
        if let Some((key, candidates)) = parse_entry(line) {
            let map = if okuri_ari {
                &mut index.okuri_ari
            } else {
                &mut index.okuri_nasi
            };
            map.entry(key).or_default().extend(candidates);
        } else {
            skipped_lines += 1;
            warn!(
                line = line_no + 1,
                "skipping malformed SKK dictionary entry"
            );
        }
    }
    if skipped_lines == bytes.split(|byte| *byte == b'\n').count() {
        return Err(ParseError::Undecodable);
    }
    Ok(ParseReport {
        index,
        encoding,
        skipped_lines,
    })
}

fn decode_line(raw: &[u8], encoding: Encoding) -> (String, bool) {
    let (text, errors) = match encoding {
        Encoding::Utf8 => UTF_8.decode_without_bom_handling(raw),
        Encoding::EucJp => EUC_JP.decode_without_bom_handling(raw),
    };
    (text.into_owned(), errors)
}

fn coding_header(bytes: &[u8]) -> Option<Encoding> {
    let first = bytes.split(|b| *b == b'\n').next()?;
    let line = String::from_utf8_lossy(first).to_ascii_lowercase();
    if !line.contains("coding:") {
        return None;
    }
    if line.contains("euc-jp") || line.contains("euc_jp") {
        Some(Encoding::EucJp)
    } else if line.contains("utf-8") || line.contains("utf8") {
        Some(Encoding::Utf8)
    } else {
        None
    }
}

fn parse_entry(line: &str) -> Option<(String, Vec<Candidate>)> {
    let (key, rest) = line.split_once(char::is_whitespace)?;
    let key = key.trim();
    let rest = rest.trim_start();
    if key.is_empty() || !rest.starts_with('/') {
        return None;
    }
    let mut candidates = Vec::new();
    for raw in split_candidates(rest) {
        if raw.is_empty() {
            continue;
        }
        let (word, annotation) = split_annotation(&raw);
        let text = decode_concat(word);
        let annotation = annotation.map(decode_concat);
        if !text.is_empty() {
            candidates.push(Candidate { text, annotation });
        }
    }
    if candidates.is_empty() {
        None
    } else {
        Some((key.to_owned(), candidates))
    }
}

fn split_annotation(raw: &str) -> (&str, Option<&str>) {
    let mut in_quote = false;
    let mut in_concat = false;
    let mut escaped = false;
    for (index, ch) in raw.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_quote && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_quote = !in_quote;
            continue;
        }
        if !in_quote && ch == '(' {
            in_concat = true;
            continue;
        }
        if !in_quote && ch == ')' {
            in_concat = false;
            continue;
        }
        if ch == ';' && !in_concat && !in_quote {
            return (&raw[..index], Some(&raw[index + 1..]));
        }
    }
    (raw, None)
}

fn split_candidates(rest: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut in_concat = false;
    let mut in_quote = false;
    let mut escaped = false;
    let mut current = String::new();
    let chars: Vec<char> = rest.chars().collect();
    for ch in chars {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if in_quote && ch == '\\' {
            current.push(ch);
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_quote = !in_quote;
            current.push(ch);
            continue;
        }
        if !in_quote && ch == '(' {
            in_concat = true;
            current.push(ch);
            continue;
        }
        if !in_quote && ch == ')' {
            in_concat = false;
            current.push(ch);
            continue;
        }
        if ch == '/' && !in_quote && !in_concat {
            result.push(std::mem::take(&mut current));
        } else {
            current.push(ch);
        }
    }
    result
}

pub(crate) fn format_entry(key: &str, candidates: &[Candidate]) -> String {
    let mut line = format!("{key} /");
    for candidate in candidates {
        line.push_str(&crate::escape::encode_candidate(&candidate.text));
        if let Some(annotation) = &candidate.annotation {
            line.push(';');
            line.push_str(&crate::escape::encode_candidate(annotation));
        }
        line.push('/');
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::EUC_JP;

    #[test]
    fn parses_utf8_and_sections_annotations() {
        let text = ";; coding: utf-8\n;; okuri-ari entries.\nおくr /送/贈/\n;; okuri-nasi entries.\nかんじ /漢字/感じ;こころ/\n";
        let parsed = parse_dictionary(text.as_bytes(), None).unwrap();
        assert_eq!(parsed.encoding, Encoding::Utf8);
        assert_eq!(parsed.index.lookup("おくr", Some('r'))[0].text, "送");
        assert_eq!(
            parsed.index.lookup("かんじ", None)[1].annotation.as_deref(),
            Some("こころ")
        );
    }

    #[test]
    fn detects_euc_jp_and_skips_bad_entry() {
        let source = ";; -*- coding: euc-jp -*-\nかんじ /漢字/\n";
        let (bytes, _, errors) = EUC_JP.encode(source);
        assert!(!errors);
        let parsed = parse_dictionary(&bytes, None).unwrap();
        assert_eq!(parsed.encoding, Encoding::EucJp);
        assert_eq!(parsed.index.lookup("かんじ", None)[0].text, "漢字");
    }

    #[test]
    fn parses_concat_slashes_and_annotations() {
        let parsed =
            parse_dictionary("word /(concat \"x\\057y\\073z\")/".as_bytes(), None).unwrap();
        let cand = &parsed.index.lookup("word", None)[0];
        assert_eq!(cand.text, "x/y;z");
        assert_eq!(cand.annotation, None);
    }

    #[test]
    fn preserves_ideographic_space_candidates() {
        let parsed = parse_dictionary("sp /　;全角スペース/".as_bytes(), None).unwrap();
        let candidate = &parsed.index.lookup("sp", None)[0];
        assert_eq!(candidate.text, "　");
        assert_eq!(candidate.annotation.as_deref(), Some("全角スペース"));
    }
}
