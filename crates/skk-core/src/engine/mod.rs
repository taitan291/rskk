use crate::{
    KanaTable, convert_number_candidate, convert_number_key, to_hankaku_katakana, to_katakana,
    to_zenkaku,
};

mod types;

pub use types::{
    Candidate, CandidateWindow, Config, Dictionary, InputMode, Key, KeyEvent, Output, Phase,
    Preedit, PreeditStyle, StyledRange,
};

/// State of the dictionary-registration sub-prompt.
#[derive(Default)]
struct Registration {
    word: String,
    roman: String,
    key: String,
    okuri: Option<char>,
}

struct CompletionState {
    prefix: String,
    words: Vec<String>,
    selected: usize,
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
    registration: Registration,
    completion: Option<CompletionState>,
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
            registration: Registration::default(),
            completion: None,
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
        self.registration.word.clear();
        self.registration.roman.clear();
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
        if ev.key != Key::Tab {
            self.completion = None;
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
            Key::Backspace | Key::Ctrl('h') => self.delete_char(),
            Key::Escape => {
                self.clear_input();
                self.phase = Phase::Escape;
                true
            }
            Key::Enter => {
                let had_input = self.phase != Phase::Input
                    || !self.reading.is_empty()
                    || !self.roman.is_empty()
                    || !self.okuri_text.is_empty()
                    || self.number_digits.is_some();
                commit = self.kakutei();
                if had_input && !self.config.egg_like_newline {
                    commit.get_or_insert_with(String::new).push('\n');
                }
                had_input
            }
            Key::Space => {
                if self.mode == InputMode::Zenkaku {
                    commit = Some("　".into());
                } else if self.phase == Phase::Henkan {
                    self.next_candidate();
                } else {
                    self.flush_roman();
                    self.begin_conversion();
                }
                true
            }
            Key::Tab => self.complete_reading(ev.shift),
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
                let selects_candidate = self
                    .config
                    .select_candidate_keys
                    .find(ch)
                    .filter(|&index| index < self.candidates.len());
                if let Some(index) = selects_candidate {
                    self.selected = index;
                } else {
                    commit = self.kakutei();
                    if let Some(text) = self.process_char(ch, ev.shift) {
                        commit.get_or_insert_with(String::new).push_str(&text);
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
        self.registration.word.clear();
        self.registration.roman.clear();
        self.completion = None;
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
            self.registration.key = self.lookup_key.clone();
            self.registration.okuri = self.lookup_okuri;
            self.registration.word.clear();
            self.registration.roman.clear();
            self.phase = Phase::Register;
        } else {
            self.phase = Phase::Henkan;
        }
    }

    fn complete_reading(&mut self, backwards: bool) -> bool {
        let supported_phase = self.phase == Phase::OkuriNasi
            || (self.phase == Phase::Input && self.mode == InputMode::Abbrev);
        if !supported_phase {
            return false;
        }
        let flushed_roman = !self.roman.is_empty();
        self.flush_roman();
        if self.reading.is_empty() {
            return flushed_roman;
        }

        if let Some(completion) = &mut self.completion
            && (completion.prefix == self.reading
                || completion.words[completion.selected] == self.reading)
        {
            completion.selected = if backwards {
                completion
                    .selected
                    .checked_sub(1)
                    .unwrap_or(completion.words.len() - 1)
            } else {
                (completion.selected + 1) % completion.words.len()
            };
            self.reading = completion.words[completion.selected].clone();
            return true;
        }

        let prefix = self.reading.clone();
        let words = self.dict.complete(&prefix);
        if words.is_empty() {
            return flushed_roman;
        }
        let selected = if backwards { words.len() - 1 } else { 0 };
        self.reading = words[selected].clone();
        self.completion = Some(CompletionState {
            prefix,
            words,
            selected,
        });
        true
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
                self.registration.key = self.lookup_key.clone();
                self.registration.okuri = self.lookup_okuri;
                self.registration.word.clear();
                self.registration.roman.clear();
                self.phase = Phase::Register;
            } else if self.selected >= self.candidates.len() {
                self.selected = self.candidates.len() - 1;
            }
        }
    }

    fn kakutei(&mut self) -> Option<String> {
        if self.phase == Phase::Register {
            self.flush_registration_roman();
            if self.registration.word.is_empty() {
                self.phase = Phase::OkuriNasi;
                return None;
            }
            let candidate = Candidate::new(self.registration.word.clone());
            self.dict
                .learn(&self.registration.key, self.registration.okuri, &candidate);
            let mut word = self.registration.word.clone();
            if self.registration.okuri.is_some() {
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
                self.registration.word.clear();
                self.registration.roman.clear();
                self.phase = Phase::OkuriNasi;
                self.roman.clear();
            }
        } else {
            self.clear_input();
            self.phase = Phase::Input;
        }
    }

    fn delete_char(&mut self) -> bool {
        if self.phase == Phase::Henkan {
            self.candidates.clear();
            self.phase = Phase::OkuriNasi;
        }
        if self.phase == Phase::Register {
            if !self.registration.roman.is_empty() {
                self.registration.roman.pop();
            } else if !self.registration.word.is_empty() {
                self.registration.word.pop();
            }
            return true;
        }
        if !self.roman.is_empty() {
            self.roman.pop();
            return true;
        }
        if self.phase == Phase::Input && self.reading.is_empty() && self.okuri_text.is_empty() {
            return false;
        }
        if !self.okuri_text.is_empty() {
            self.okuri_text.pop();
        } else if !self.reading.is_empty() {
            self.reading.pop();
        }
        if self.reading.is_empty() {
            self.phase = Phase::Input;
            self.okuri_consonant = None;
            self.okuri_text.clear();
        }
        true
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
            self.registration.roman.push(ch.to_ascii_lowercase());
        } else {
            self.flush_registration_roman();
            self.registration.word.push(ch);
            return None;
        }
        while !self.registration.roman.is_empty() {
            let chars: Vec<char> = self.registration.roman.chars().collect();
            if self.registration.roman == "n'" || self.registration.roman == "nn" {
                self.registration.word.push('ん');
                self.registration.roman.clear();
                continue;
            }
            if chars.len() >= 2
                && chars[0] == 'n'
                && !matches!(chars[1], 'a' | 'i' | 'u' | 'e' | 'o' | 'y' | 'n' | '\'')
            {
                self.registration.word.push('ん');
                self.registration.roman.remove(0);
                continue;
            }
            if chars.len() >= 2
                && chars[0] == chars[1]
                && chars[0].is_ascii_alphabetic()
                && !matches!(chars[0], 'a' | 'i' | 'u' | 'e' | 'o' | 'n')
            {
                self.registration.word.push('っ');
                self.registration.roman.remove(0);
                continue;
            }
            if let Some(value) = self.table.exact(&self.registration.roman) {
                if self.table.has_longer_prefix(&self.registration.roman) {
                    break;
                }
                let value = value.to_owned();
                self.registration.roman.clear();
                self.registration.word.push_str(&value);
            } else if self.table.has_prefix(&self.registration.roman) {
                break;
            } else if let Some((key, value)) = self.table.longest_prefix(&self.registration.roman) {
                let key = key.to_owned();
                let value = value.to_owned();
                self.registration.roman.drain(..key.len());
                self.registration.word.push_str(&value);
            } else {
                let invalid = self.registration.roman.remove(0);
                if self.config.accept_illegal_result {
                    self.registration.word.push(invalid);
                }
            }
        }
        None
    }

    fn flush_registration_roman(&mut self) {
        while !self.registration.roman.is_empty() {
            if self.registration.roman == "n" {
                self.registration.word.push('ん');
                self.registration.roman.clear();
                continue;
            }
            if let Some(value) = self.table.exact(&self.registration.roman) {
                let value = value.to_owned();
                self.registration.word.push_str(&value);
                self.registration.roman.clear();
                continue;
            }
            if let Some((key, value)) = self.table.longest_prefix(&self.registration.roman) {
                let key = key.to_owned();
                let value = value.to_owned();
                self.registration.roman.drain(..key.len());
                self.registration.word.push_str(&value);
                continue;
            }
            if self.config.accept_illegal_result {
                self.registration.word.push_str(&self.registration.roman);
            }
            self.registration.roman.clear();
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
                preedit.text.push_str(&self.registration.word);
                preedit.text.push_str(&self.registration.roman);
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
mod tests;
