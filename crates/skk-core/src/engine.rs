use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    KanaTable, convert_number_candidate, convert_number_key, to_hankaku_katakana, to_katakana,
    to_zenkaku,
};

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

/// Deterministic SKK state machine. Dictionary mutation is delegated through `Dictionary`.
pub struct Engine {
    config: Config,
    table: KanaTable,
    dict: Box<dyn Dictionary>,
    mode: InputMode,
    last_mode: InputMode,
    phase: Phase,
    reading: String,
    roman: String,
    okuri_consonant: Option<char>,
    okuri_text: String,
    candidates: Vec<Candidate>,
    selected: usize,
    lookup_key: String,
    lookup_okuri: Option<char>,
    number_digits: Option<String>,
    registration_word: String,
    registration_roman: String,
    registration_key: String,
    registration_okuri: Option<char>,
}

impl Engine {
    pub fn new(config: Config, dict: Box<dyn Dictionary>) -> Self {
        Self::with_kana_table(config, KanaTable::standard(), dict)
    }

    pub fn with_kana_table(config: Config, table: KanaTable, dict: Box<dyn Dictionary>) -> Self {
        Self {
            config,
            table,
            dict,
            mode: InputMode::Disabled,
            last_mode: InputMode::Hira,
            phase: Phase::Input,
            reading: String::new(),
            roman: String::new(),
            okuri_consonant: None,
            okuri_text: String::new(),
            candidates: Vec::new(),
            selected: 0,
            lookup_key: String::new(),
            lookup_okuri: None,
            number_digits: None,
            registration_word: String::new(),
            registration_roman: String::new(),
            registration_key: String::new(),
            registration_okuri: None,
        }
    }

    pub fn mode(&self) -> InputMode {
        self.mode
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn reset(&mut self) {
        self.clear_input();
        self.phase = Phase::Input;
        self.registration_word.clear();
        self.registration_roman.clear();
    }

    pub fn handle_key(&mut self, ev: KeyEvent) -> Output {
        let toggle = ev.logo && ev.key == Key::Space;
        let activate = toggle || (self.mode == InputMode::Disabled && ev.key == Key::Ctrl('j'));
        if activate {
            if self.mode == InputMode::Disabled {
                self.enable();
            } else {
                self.disable();
            }
            return self.output(true, None);
        }
        if self.mode == InputMode::Disabled {
            return self.output(false, None);
        }
        if self.phase == Phase::Escape {
            self.phase = Phase::Input;
            return self.output(false, None);
        }

        let mut commit = None;
        let consumed = match ev.key.clone() {
            Key::Ctrl('g') => {
                self.cancel();
                true
            }
            Key::Ctrl('j') => {
                commit = self.kakutei();
                true
            }
            Key::Backspace | Key::Ctrl('h') => {
                self.delete_char();
                true
            }
            Key::Escape => {
                self.clear_input();
                self.phase = Phase::Escape;
                true
            }
            Key::Enter => {
                commit = self.kakutei();
                if !self.config.egg_like_newline {
                    commit.get_or_insert_with(String::new).push('\n');
                }
                true
            }
            Key::Space => {
                if self.phase == Phase::Henkan {
                    self.next_candidate();
                } else {
                    self.flush_roman();
                    self.begin_conversion();
                }
                true
            }
            Key::Tab => false,
            Key::Ctrl('q') => {
                self.flush_roman();
                self.change_mode(InputMode::Hankata);
                true
            }
            Key::Char('q') if self.phase != Phase::Henkan && self.mode != InputMode::Abbrev => {
                self.flush_roman();
                self.mode = match self.mode {
                    InputMode::Kata => InputMode::Hira,
                    _ => InputMode::Kata,
                };
                self.last_mode = self.mode;
                true
            }
            Key::Char('q') if self.phase != Phase::Henkan && self.mode == InputMode::Abbrev => {
                commit = self.kakutei();
                self.change_mode(InputMode::Hira);
                true
            }
            Key::Char('q') if self.phase == Phase::Henkan => {
                if self.config.register_convert_result {
                    self.learn_machine_conversion();
                }
                let mut text = to_katakana(&self.reading);
                text.push_str(&self.mode_text(&self.okuri_text));
                commit = Some(text);
                self.clear_input();
                true
            }
            Key::Char('l')
                if self.phase != Phase::Henkan
                    && self.mode != InputMode::Abbrev
                    && self.roman.is_empty()
                    && self.reading.is_empty() =>
            {
                self.disable();
                true
            }
            Key::Char('L')
                if self.phase != Phase::Henkan
                    && self.mode != InputMode::Abbrev
                    && self.roman.is_empty() =>
            {
                self.change_mode(InputMode::Zenkaku);
                true
            }
            Key::Char('X') if self.phase == Phase::Henkan => {
                self.purge_candidate();
                true
            }
            Key::Char('x') if self.phase == Phase::Henkan => {
                self.previous_candidate();
                true
            }
            Key::Char(ch) if self.phase == Phase::Henkan => {
                if let Some(index) = self.config.select_candidate_keys.find(ch) {
                    if index < self.candidates.len() {
                        self.selected = index;
                    } else {
                        commit = self.kakutei();
                        let next = self.process_char(ch, ev.shift);
                        if let Some(text) = next {
                            commit.get_or_insert_with(String::new).push_str(&text);
                        }
                    }
                } else {
                    commit = self.kakutei();
                    let next = self.process_char(ch, ev.shift);
                    if let Some(next_commit) = next {
                        commit
                            .get_or_insert_with(String::new)
                            .push_str(&next_commit);
                    }
                }
                true
            }
            Key::Char(ch) => {
                commit = self.process_char(ch, ev.shift);
                true
            }
            Key::Ctrl(_) => false,
        };
        self.output(consumed, commit)
    }

    fn enable(&mut self) {
        self.mode = if self.config.keep_mode {
            self.last_mode
        } else {
            InputMode::Hira
        };
        if self.mode == InputMode::Disabled {
            self.mode = InputMode::Hira;
        }
        self.phase = Phase::Input;
    }

    fn disable(&mut self) {
        if self.mode != InputMode::Disabled {
            self.last_mode = self.mode;
        }
        self.mode = InputMode::Disabled;
        self.clear_input();
        self.phase = Phase::Input;
    }

    fn change_mode(&mut self, mode: InputMode) {
        self.last_mode = mode;
        self.mode = mode;
    }

    fn clear_input(&mut self) {
        self.reading.clear();
        self.roman.clear();
        self.okuri_consonant = None;
        self.okuri_text.clear();
        self.candidates.clear();
        self.selected = 0;
        self.lookup_key.clear();
        self.lookup_okuri = None;
        self.number_digits = None;
        self.registration_word.clear();
        self.registration_roman.clear();
    }

    fn process_char(&mut self, ch: char, shifted: bool) -> Option<String> {
        if self.phase == Phase::Register {
            return self.registration_key(ch);
        }
        if self.mode == InputMode::Zenkaku {
            let input = ch.to_string();
            return Some(to_zenkaku(&input));
        }
        if self.mode == InputMode::Abbrev {
            if ch == '/' && self.reading.is_empty() {
                return None;
            }
            self.reading.push(ch);
            return None;
        }
        if ch == '/'
            && self.phase == Phase::Input
            && self.reading.is_empty()
            && self.roman.is_empty()
        {
            self.change_mode(InputMode::Abbrev);
            return None;
        }
        if ch.is_ascii_digit() {
            if self.phase == Phase::OkuriAri {
                self.okuri_text.push(ch);
                self.begin_okuri_conversion();
            } else {
                self.reading.push(ch);
            }
            return None;
        }
        if self.phase == Phase::Henkan {
            let mut commit = self.kakutei();
            let following = self.process_char(ch, shifted);
            if let Some(text) = following {
                commit.get_or_insert_with(String::new).push_str(&text);
            }
            return commit;
        }

        let upper = shifted || ch.is_ascii_uppercase();
        let mut actual = ch;
        if upper {
            actual = self
                .config
                .lowercase_map
                .get(&ch.to_string())
                .and_then(|s| s.chars().next())
                .unwrap_or_else(|| ch.to_ascii_lowercase());
            self.flush_roman();
            if self.phase == Phase::Input {
                self.phase = Phase::OkuriNasi;
            } else if self.phase == Phase::OkuriNasi {
                self.phase = Phase::OkuriAri;
                self.okuri_consonant = Some(actual);
            }
        }
        if self.phase == Phase::OkuriAri
            && self.okuri_consonant.is_none()
            && actual.is_ascii_alphabetic()
        {
            self.okuri_consonant = Some(actual.to_ascii_lowercase());
        }
        if self.mode == InputMode::Abbrev {
            self.reading.push(actual);
            return None;
        }
        self.feed_roman(actual.to_ascii_lowercase());
        None
    }

    fn feed_roman(&mut self, ch: char) {
        self.roman.push(ch);
        loop {
            if self.roman.is_empty() {
                return;
            }
            let chars: Vec<char> = self.roman.chars().collect();
            if self.roman == "n'" {
                self.append_kana("ん");
                self.roman.clear();
                continue;
            }
            if self.roman == "nn" {
                self.append_kana("ん");
                self.roman.clear();
                continue;
            }
            if chars[0] == 'n'
                && chars.len() >= 2
                && !matches!(chars[1], 'a' | 'i' | 'u' | 'e' | 'o' | 'y' | 'n' | '\'')
            {
                self.append_kana("ん");
                self.roman.remove(0);
                continue;
            }
            if chars.len() >= 2
                && chars[0] == chars[1]
                && chars[0].is_ascii_alphabetic()
                && !matches!(chars[0], 'a' | 'i' | 'u' | 'e' | 'o' | 'n')
            {
                self.append_kana("っ");
                self.roman.remove(0);
                continue;
            }
            if let Some(value) = self.table.exact(&self.roman) {
                if self.table.has_longer_prefix(&self.roman) {
                    return;
                }
                let value = value.to_owned();
                self.roman.clear();
                self.append_kana(&value);
                continue;
            }
            if self.table.has_prefix(&self.roman) {
                return;
            }
            if let Some((key, value)) = self.table.longest_prefix(&self.roman) {
                let key = key.to_owned();
                let value = value.to_owned();
                self.roman.drain(..key.len());
                self.append_kana(&value);
                continue;
            }
            if self.config.accept_illegal_result {
                let invalid = self.roman.remove(0);
                self.reading.push(invalid);
            } else {
                self.roman.remove(0);
            }
        }
    }

    fn append_kana(&mut self, kana: &str) {
        let kana = kana.to_owned();
        if self.phase == Phase::OkuriAri {
            self.okuri_text.push_str(&kana);
            if self.config.immediately_okuri_convert && self.okuri_text == "っ" {
                self.okuri_consonant = Some('t');
                self.begin_okuri_conversion();
            }
            if !self.okuri_text.is_empty() && self.phase == Phase::OkuriAri {
                self.begin_okuri_conversion();
            }
        } else {
            self.reading.push_str(&kana);
        }
    }

    fn flush_roman(&mut self) {
        while !self.roman.is_empty() {
            if self.roman == "n" {
                self.roman.clear();
                self.append_kana("ん");
                continue;
            }
            if let Some(value) = self.table.exact(&self.roman) {
                let value = value.to_owned();
                self.roman.clear();
                self.append_kana(&value);
                continue;
            }
            if let Some((key, value)) = self.table.longest_prefix(&self.roman) {
                let key = key.to_owned();
                let value = value.to_owned();
                self.roman.drain(..key.len());
                self.append_kana(&value);
                continue;
            }
            if self.config.accept_illegal_result {
                self.reading.push_str(&self.roman);
            }
            self.roman.clear();
        }
    }

    fn begin_conversion(&mut self) {
        if self.mode == InputMode::Abbrev && self.phase == Phase::Input && !self.reading.is_empty()
        {
            self.lookup_key = self.reading.clone();
            self.lookup_okuri = None;
            self.number_digits = None;
            self.load_candidates();
            return;
        }
        if self.phase != Phase::OkuriNasi || self.reading.is_empty() {
            return;
        }
        let (key, digits) = convert_number_key(&self.reading);
        self.lookup_key = key;
        self.lookup_okuri = None;
        self.number_digits = digits;
        self.load_candidates();
    }

    fn begin_okuri_conversion(&mut self) {
        let Some(okuri) = self.okuri_consonant else {
            return;
        };
        if self.reading.is_empty() {
            return;
        }
        let raw = format!("{}{}", self.reading, okuri);
        let (key, digits) = convert_number_key(&raw);
        self.lookup_key = key;
        self.lookup_okuri = Some(okuri);
        self.number_digits = digits;
        self.load_candidates();
    }

    fn load_candidates(&mut self) {
        let mut candidates = self.dict.lookup(&self.lookup_key, self.lookup_okuri);
        if let Some(digits) = self.number_digits.as_deref() {
            candidates = candidates
                .into_iter()
                .map(|mut candidate| {
                    candidate.text = convert_number_candidate(&candidate.text, digits);
                    candidate
                })
                .collect();
        }
        self.candidates = candidates;
        self.selected = 0;
        if self.candidates.is_empty() {
            self.registration_key = self.lookup_key.clone();
            self.registration_okuri = self.lookup_okuri;
            self.registration_word.clear();
            self.registration_roman.clear();
            self.phase = Phase::Register;
        } else {
            self.phase = Phase::Henkan;
        }
    }

    fn next_candidate(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.candidates.len();
    }

    fn previous_candidate(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        self.selected = self
            .selected
            .checked_sub(1)
            .unwrap_or(self.candidates.len() - 1);
    }

    fn purge_candidate(&mut self) {
        if let Some(candidate) = self.candidates.get(self.selected).cloned() {
            self.dict
                .purge(&self.lookup_key, self.lookup_okuri, &candidate);
            self.candidates.remove(self.selected);
            if self.candidates.is_empty() {
                self.registration_key = self.lookup_key.clone();
                self.registration_okuri = self.lookup_okuri;
                self.registration_word.clear();
                self.registration_roman.clear();
                self.phase = Phase::Register;
            } else if self.selected >= self.candidates.len() {
                self.selected = self.candidates.len() - 1;
            }
        }
    }

    fn kakutei(&mut self) -> Option<String> {
        if self.phase == Phase::Register {
            self.flush_registration_roman();
            if self.registration_word.is_empty() {
                self.phase = Phase::OkuriNasi;
                return None;
            }
            let candidate = Candidate::new(self.registration_word.clone());
            self.dict
                .learn(&self.registration_key, self.registration_okuri, &candidate);
            let mut word = self.registration_word.clone();
            if self.registration_okuri.is_some() {
                word.push_str(&self.mode_text(&self.okuri_text));
            }
            self.clear_input();
            self.phase = Phase::Input;
            return Some(word);
        }
        if self.phase == Phase::Henkan {
            let selected = self.candidates.get(self.selected)?.clone();
            let mut commit = selected.text.clone();
            commit.push_str(&self.mode_text(&self.okuri_text));
            if self.config.immediately_dictionary_rw && self.number_digits.is_none() {
                self.dict
                    .learn(&self.lookup_key, self.lookup_okuri, &selected);
            }
            self.clear_input();
            self.phase = Phase::Input;
            return Some(commit);
        }
        self.flush_roman();
        let mut text = self.mode_text(&self.reading);
        text.push_str(&self.mode_text(&self.okuri_text));
        if self.mode == InputMode::Abbrev {
            text = self.reading.clone();
        }
        if self.config.register_convert_result
            && !text.is_empty()
            && self.mode != InputMode::Hira
            && self.mode != InputMode::Abbrev
        {
            let key = self.reading.clone();
            let value = Candidate::new(text.clone());
            self.dict.learn(&key, None, &value);
        }
        self.clear_input();
        self.phase = Phase::Input;
        if text.is_empty() { None } else { Some(text) }
    }

    fn cancel(&mut self) {
        if self.phase == Phase::Henkan || self.phase == Phase::Register {
            if self.config.immediately_cancel {
                self.clear_input();
                self.phase = Phase::Input;
            } else {
                self.candidates.clear();
                self.registration_word.clear();
                self.registration_roman.clear();
                self.phase = Phase::OkuriNasi;
                self.roman.clear();
            }
        } else {
            self.clear_input();
            self.phase = Phase::Input;
        }
    }

    fn delete_char(&mut self) {
        if self.phase == Phase::Henkan {
            self.candidates.clear();
            self.phase = Phase::OkuriNasi;
        }
        if !self.roman.is_empty() {
            self.roman.pop();
            return;
        }
        if self.phase == Phase::Register {
            if !self.registration_roman.is_empty() {
                self.registration_roman.pop();
                return;
            }
            if !self.registration_word.is_empty() {
                self.registration_word.pop();
            }
            return;
        }
        if !self.okuri_text.is_empty() {
            self.okuri_text.pop();
        } else {
            self.reading.pop();
        }
        if self.reading.is_empty() {
            self.phase = Phase::Input;
        }
    }

    fn learn_machine_conversion(&mut self) -> String {
        let converted = to_katakana(&self.reading);
        if !converted.is_empty() {
            self.dict
                .learn(&self.reading, None, &Candidate::new(converted.clone()));
        }
        converted
    }

    fn mode_text(&self, text: &str) -> String {
        match self.mode {
            InputMode::Kata => to_katakana(text),
            InputMode::Hankata => to_hankaku_katakana(&to_katakana(text)),
            _ => text.to_owned(),
        }
    }

    fn registration_key(&mut self, ch: char) -> Option<String> {
        if ch.is_ascii_alphabetic() {
            self.registration_roman.push(ch.to_ascii_lowercase());
        } else {
            self.flush_registration_roman();
            self.registration_word.push(ch);
            return None;
        }
        while !self.registration_roman.is_empty() {
            let chars: Vec<char> = self.registration_roman.chars().collect();
            if self.registration_roman == "n'" || self.registration_roman == "nn" {
                self.registration_word.push('ん');
                self.registration_roman.clear();
                continue;
            }
            if chars.len() >= 2
                && chars[0] == 'n'
                && !matches!(chars[1], 'a' | 'i' | 'u' | 'e' | 'o' | 'y' | 'n' | '\'')
            {
                self.registration_word.push('ん');
                self.registration_roman.remove(0);
                continue;
            }
            if chars.len() >= 2
                && chars[0] == chars[1]
                && chars[0].is_ascii_alphabetic()
                && !matches!(chars[0], 'a' | 'i' | 'u' | 'e' | 'o' | 'n')
            {
                self.registration_word.push('っ');
                self.registration_roman.remove(0);
                continue;
            }
            if let Some(value) = self.table.exact(&self.registration_roman) {
                if self.table.has_longer_prefix(&self.registration_roman) {
                    break;
                }
                let value = value.to_owned();
                self.registration_roman.clear();
                self.registration_word.push_str(&value);
            } else if self.table.has_prefix(&self.registration_roman) {
                break;
            } else if let Some((key, value)) = self.table.longest_prefix(&self.registration_roman) {
                let key = key.to_owned();
                let value = value.to_owned();
                self.registration_roman.drain(..key.len());
                self.registration_word.push_str(&value);
            } else {
                let invalid = self.registration_roman.remove(0);
                if self.config.accept_illegal_result {
                    self.registration_word.push(invalid);
                }
            }
        }
        None
    }

    fn flush_registration_roman(&mut self) {
        while !self.registration_roman.is_empty() {
            if self.registration_roman == "n" {
                self.registration_word.push('ん');
                self.registration_roman.clear();
                continue;
            }
            if let Some(value) = self.table.exact(&self.registration_roman) {
                let value = value.to_owned();
                self.registration_word.push_str(&value);
                self.registration_roman.clear();
                continue;
            }
            if let Some((key, value)) = self.table.longest_prefix(&self.registration_roman) {
                let key = key.to_owned();
                let value = value.to_owned();
                self.registration_roman.drain(..key.len());
                self.registration_word.push_str(&value);
                continue;
            }
            if self.config.accept_illegal_result {
                self.registration_word.push_str(&self.registration_roman);
            }
            self.registration_roman.clear();
        }
    }

    fn output(&self, consumed: bool, commit: Option<String>) -> Output {
        let mut preedit = Preedit::default();
        match self.phase {
            Phase::Input => {
                preedit.text.push_str(&self.mode_text(&self.reading));
                preedit.text.push_str(&self.roman);
            }
            Phase::OkuriNasi | Phase::OkuriAri => {
                let start = preedit.text.len();
                preedit.text.push_str(&self.config.marker_henkan);
                preedit.styles.push(StyledRange {
                    start,
                    end: preedit.text.len(),
                    style: PreeditStyle::Underline,
                });
                preedit.text.push_str(&self.mode_text(&self.reading));
                if self.phase == Phase::OkuriAri {
                    preedit.text.push_str(&self.mode_text(&self.okuri_text));
                    preedit.text.push_str(&self.roman);
                } else {
                    preedit.text.push_str(&self.roman);
                }
            }
            Phase::Henkan => {
                let start = preedit.text.len();
                preedit.text.push_str(&self.config.marker_henkan_select);
                preedit.styles.push(StyledRange {
                    start,
                    end: preedit.text.len(),
                    style: PreeditStyle::Highlight,
                });
                if let Some(candidate) = self.candidates.get(self.selected) {
                    let start = preedit.text.len();
                    preedit.text.push_str(&candidate.text);
                    preedit.text.push_str(&self.mode_text(&self.okuri_text));
                    preedit.styles.push(StyledRange {
                        start,
                        end: preedit.text.len(),
                        style: PreeditStyle::Highlight,
                    });
                }
            }
            Phase::Register => {
                preedit.text.push_str("辞書登録: ");
                preedit.text.push_str(&self.registration_word);
                preedit.text.push_str(&self.registration_roman);
            }
            Phase::Escape => {}
        }
        preedit.cursor = preedit.text.len();
        let candidates = (self.phase == Phase::Henkan
            && self.candidates.len() > self.config.show_candidates_count)
            .then(|| CandidateWindow {
                items: self
                    .candidates
                    .iter()
                    .map(|candidate| {
                        let mut item =
                            format!("{}{}", candidate.text, self.mode_text(&self.okuri_text));
                        if let Some(annotation) = &candidate.annotation {
                            item.push_str("  ");
                            item.push_str(annotation);
                        }
                        item
                    })
                    .collect(),
                selected: self.selected,
                select_keys: self.config.select_candidate_keys.clone(),
            });
        Output {
            consumed,
            commit,
            preedit,
            candidates,
            mode: self.mode,
            phase: self.phase,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct MemoryDict(RefCell<HashMap<String, Vec<Candidate>>>);
    impl MemoryDict {
        fn add(&self, key: &str, candidates: &[&str]) {
            self.0.borrow_mut().insert(
                key.into(),
                candidates.iter().map(|s| Candidate::new(*s)).collect(),
            );
        }
    }
    impl Dictionary for MemoryDict {
        fn lookup(&self, key: &str, _okuri: Option<char>) -> Vec<Candidate> {
            self.0.borrow().get(key).cloned().unwrap_or_default()
        }
        fn learn(&mut self, key: &str, _okuri: Option<char>, candidate: &Candidate) {
            self.0
                .borrow_mut()
                .entry(key.into())
                .or_default()
                .insert(0, candidate.clone());
        }
        fn purge(&mut self, key: &str, _okuri: Option<char>, candidate: &Candidate) {
            if let Some(v) = self.0.borrow_mut().get_mut(key) {
                v.retain(|c| c != candidate);
            }
        }
    }
    fn engine(pairs: &[(&str, &[&str])]) -> Engine {
        let dict = MemoryDict::default();
        for (k, v) in pairs {
            dict.add(k, v);
        }
        let mut engine = Engine::new(Config::default(), Box::new(dict));
        engine.handle_key(KeyEvent::ctrl('j'));
        engine
    }
    fn key(engine: &mut Engine, ch: char) -> Output {
        engine.handle_key(KeyEvent::new(Key::Char(ch)))
    }
    fn upper(engine: &mut Engine, ch: char) -> Output {
        engine.handle_key(KeyEvent::shifted(ch))
    }
    fn type_text(engine: &mut Engine, text: &str) {
        for ch in text.chars() {
            key(engine, ch);
        }
    }

    #[test]
    fn romaji_rules() {
        let mut e = engine(&[]);
        type_text(&mut e, "ka");
        assert_eq!(e.reading, "か");
        e.clear_input();
        type_text(&mut e, "kka");
        assert_eq!(e.reading, "っか");
        e.clear_input();
        type_text(&mut e, "nn");
        assert_eq!(e.reading, "ん");
        e.clear_input();
        type_text(&mut e, "kya");
        assert_eq!(e.reading, "きゃ");
        e.clear_input();
        type_text(&mut e, "nka");
        assert_eq!(e.reading, "んか");
        e.clear_input();
        type_text(&mut e, "n'");
        assert_eq!(e.reading, "ん");
    }

    #[test]
    fn converts_and_commits_candidates() {
        let mut e = engine(&[("かんじ", &["漢字", "感じ"])]);
        upper(&mut e, 'K');
        type_text(&mut e, "anj i".replace(' ', "").as_str());
        assert_eq!(e.reading, "かんじ");
        let output = e.handle_key(KeyEvent::new(Key::Space));
        assert_eq!(output.preedit.text, "▼漢字");
        let output = e.handle_key(KeyEvent::new(Key::Enter));
        assert_eq!(output.commit.as_deref(), Some("漢字\n"));
    }

    #[test]
    fn okuri_ari_and_candidate_window() {
        let mut e = engine(&[
            ("おくr", &["送", "贈"]),
            ("かんじ", &["漢字", "幹事", "感じ", "監事", "完治"]),
        ]);
        upper(&mut e, 'O');
        type_text(&mut e, "ku");
        upper(&mut e, 'R');
        key(&mut e, 'u');
        assert_eq!(e.lookup_key, "おくr");
        assert_eq!(e.okuri_text, "る");
        assert_eq!(e.output(true, None).preedit.text, "▼送る");
        let mut many = engine(&[("かんじ", &["漢字", "幹事", "感じ", "監事", "完治"])]);
        upper(&mut many, 'K');
        type_text(&mut many, "anji");
        let output = many.handle_key(KeyEvent::new(Key::Space));
        assert_eq!(output.candidates.unwrap().items.len(), 5);
        let output = key(&mut many, 'f');
        assert_eq!(output.candidates.unwrap().selected, 3);
    }

    #[test]
    fn cancellation_and_modes() {
        let mut e = engine(&[("かんじ", &["漢字"])]);
        upper(&mut e, 'K');
        type_text(&mut e, "anji");
        e.handle_key(KeyEvent::new(Key::Space));
        let canceled = e.handle_key(KeyEvent::ctrl('g'));
        assert_eq!(canceled.preedit.text, "");
        key(&mut e, 'q');
        assert_eq!(e.mode, InputMode::Kata);
        let config = Config {
            keep_mode: true,
            ..Config::default()
        };
        let mut e = Engine::new(config, Box::<MemoryDict>::default());
        e.handle_key(KeyEvent::ctrl('j'));
        key(&mut e, 'q');
        e.handle_key(KeyEvent::new(Key::Char('l')));
        e.handle_key(KeyEvent::ctrl('j'));
        assert_eq!(e.mode, InputMode::Kata);
    }

    #[test]
    fn cancellation_covers_input_okuri_and_registration_phases() {
        let mut e = engine(&[]);
        type_text(&mut e, "ka");
        assert_eq!(e.handle_key(KeyEvent::ctrl('g')).preedit.text, "");

        upper(&mut e, 'K');
        type_text(&mut e, "anji");
        assert_eq!(e.phase, Phase::OkuriNasi);
        assert_eq!(e.handle_key(KeyEvent::ctrl('g')).preedit.text, "");

        upper(&mut e, 'O');
        type_text(&mut e, "ku");
        upper(&mut e, 'R');
        assert_eq!(e.phase, Phase::OkuriAri);
        assert_eq!(e.handle_key(KeyEvent::ctrl('g')).preedit.text, "");

        upper(&mut e, 'Y');
        type_text(&mut e, "omi");
        e.handle_key(KeyEvent::new(Key::Space));
        assert_eq!(e.phase, Phase::Register);
        assert_eq!(e.handle_key(KeyEvent::ctrl('g')).preedit.text, "");
        assert!(e.dict.lookup("よみ", None).is_empty());
    }

    #[test]
    fn cancel_candidate_can_restore_conversion_reading() {
        let dict = MemoryDict::default();
        dict.add("かんじ", &["漢字"]);
        let mut e = Engine::new(
            Config {
                immediately_cancel: false,
                ..Config::default()
            },
            Box::new(dict),
        );
        e.handle_key(KeyEvent::ctrl('j'));
        upper(&mut e, 'K');
        type_text(&mut e, "anji");
        e.handle_key(KeyEvent::new(Key::Space));

        let canceled = e.handle_key(KeyEvent::ctrl('g'));
        assert_eq!(e.phase, Phase::OkuriNasi);
        assert_eq!(canceled.preedit.text, "▽かんじ");
    }

    #[test]
    fn abbrev_modes_and_katakana_conversion() {
        let mut e = engine(&[("New York", &["ニューヨーク"])]);
        key(&mut e, '/');
        upper(&mut e, 'N');
        type_text(&mut e, "ew York");
        assert_eq!(e.mode, InputMode::Abbrev);
        assert_eq!(e.reading, "New York");
        let output = e.handle_key(KeyEvent::new(Key::Space));
        assert_eq!(output.preedit.text, "▼ニューヨーク");
        assert_eq!(
            e.handle_key(KeyEvent::new(Key::Enter)).commit.as_deref(),
            Some("ニューヨーク\n")
        );

        let mut e = engine(&[("かな", &["仮名"])]);
        upper(&mut e, 'K');
        type_text(&mut e, "ana");
        e.handle_key(KeyEvent::new(Key::Space));
        assert_eq!(key(&mut e, 'q').commit.as_deref(), Some("カナ"));
    }

    #[test]
    fn halfwidth_fullwidth_and_registration() {
        let mut e = engine(&[]);
        e.handle_key(KeyEvent::ctrl('q'));
        type_text(&mut e, "ga");
        assert_eq!(
            e.handle_key(KeyEvent::new(Key::Enter)).commit.as_deref(),
            Some("ｶﾞ\n")
        );

        key(&mut e, 'L');
        assert_eq!(key(&mut e, 'A').commit.as_deref(), Some("Ａ"));

        let mut e = engine(&[]);
        upper(&mut e, 'Y');
        type_text(&mut e, "omi");
        e.handle_key(KeyEvent::new(Key::Space));
        assert_eq!(e.phase, Phase::Register);
        type_text(&mut e, "gengo");
        assert_eq!(
            e.handle_key(KeyEvent::new(Key::Enter)).commit.as_deref(),
            Some("げんご\n")
        );
        assert_eq!(e.dict.lookup("よみ", None)[0].text, "げんご");
    }

    #[test]
    fn custom_table_flush_and_newline_setting() {
        let table = KanaTable::from_toml("[kana]\na = 'あ'\nai = 'あい'\n").unwrap();
        let mut e = Engine::with_kana_table(Config::default(), table, Box::<MemoryDict>::default());
        e.handle_key(KeyEvent::ctrl('j'));
        key(&mut e, 'a');
        assert_eq!(e.reading, "");
        assert_eq!(e.roman, "a");
        e.flush_roman();
        assert_eq!(e.reading, "あ");

        let config = Config {
            egg_like_newline: true,
            ..Config::default()
        };
        let mut e = Engine::new(config, Box::<MemoryDict>::default());
        e.handle_key(KeyEvent::ctrl('j'));
        type_text(&mut e, "ka");
        assert_eq!(
            e.handle_key(KeyEvent::new(Key::Enter)).commit.as_deref(),
            Some("か")
        );
    }

    #[test]
    fn number_markers_and_newline_setting() {
        let mut e = engine(&[("#", &["#0/#1/#2/#3/#8"])]);
        upper(&mut e, 'K');
        key(&mut e, '1');
        key(&mut e, '2');
        e.reading = "12".into();
        e.phase = Phase::OkuriNasi;
        let output = e.handle_key(KeyEvent::new(Key::Space));
        assert_eq!(output.preedit.text, "▼12/１２/一二/十二/12");
        let mut e = engine(&[]);
        type_text(&mut e, "ka");
        assert_eq!(
            e.handle_key(KeyEvent::new(Key::Enter)).commit.as_deref(),
            Some("か\n")
        );
    }
}
