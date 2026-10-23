use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Candidate {
    pub text: String,
    #[serde(default)]
    pub annotation: Option<String>,
}

impl Candidate {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            annotation: None,
        }
    }
}

pub trait Dictionary {
    fn lookup(&self, key: &str, okuri: Option<char>) -> Vec<Candidate>;
    /// Return longer okuri-nasi dictionary keys that start with `prefix`.
    fn complete(&self, _prefix: &str) -> Vec<String> {
        Vec::new()
    }
    fn learn(&mut self, key: &str, okuri: Option<char>, candidate: &Candidate);
    fn purge(&mut self, key: &str, okuri: Option<char>, candidate: &Candidate);
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputMode {
    #[default]
    Disabled,
    Hira,
    Kata,
    Hankata,
    Zenkaku,
    Abbrev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Input,
    OkuriNasi,
    OkuriAri,
    Henkan,
    Escape,
    Register,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Space,
    Enter,
    Backspace,
    Escape,
    Tab,
    Ctrl(char),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
}

impl KeyEvent {
    pub fn new(key: Key) -> Self {
        Self {
            key,
            shift: false,
            ctrl: false,
            alt: false,
            logo: false,
        }
    }
    pub fn ctrl(ch: char) -> Self {
        Self::new(Key::Ctrl(ch))
    }
    pub fn shifted(ch: char) -> Self {
        Self {
            key: Key::Char(ch),
            shift: true,
            ctrl: false,
            alt: false,
            logo: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreeditStyle {
    Underline,
    Highlight,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledRange {
    pub start: usize,
    pub end: usize,
    pub style: PreeditStyle,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Preedit {
    pub text: String,
    pub cursor: usize,
    pub styles: Vec<StyledRange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateWindow {
    pub items: Vec<String>,
    pub selected: usize,
    pub select_keys: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub consumed: bool,
    pub commit: Option<String>,
    pub preedit: Preedit,
    pub candidates: Option<CandidateWindow>,
    pub mode: InputMode,
    pub phase: Phase,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub keep_mode: bool,
    pub show_candidates_count: usize,
    pub select_candidate_keys: String,
    pub immediately_cancel: bool,
    pub immediately_okuri_convert: bool,
    pub immediately_dictionary_rw: bool,
    pub register_convert_result: bool,
    pub egg_like_newline: bool,
    pub marker_henkan: String,
    pub marker_henkan_select: String,
    pub kana_table: String,
    pub accept_illegal_result: bool,
    pub lowercase_map: HashMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            keep_mode: false,
            show_candidates_count: 4,
            select_candidate_keys: "asdfjkl".into(),
            immediately_cancel: true,
            immediately_okuri_convert: true,
            immediately_dictionary_rw: true,
            register_convert_result: false,
            egg_like_newline: false,
            marker_henkan: "▽".into(),
            marker_henkan_select: "▼".into(),
            kana_table: "rom".into(),
            accept_illegal_result: false,
            lowercase_map: HashMap::new(),
        }
    }
}
