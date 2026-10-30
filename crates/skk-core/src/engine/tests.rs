use super::*;
use std::{cell::RefCell, collections::HashMap};

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
    assert_eq!(
        e.handle_key(KeyEvent::new(Key::Space)).commit.as_deref(),
        Some("　")
    );

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

#[test]
fn direct_mode_passes_everything_through() {
    let mut e = engine(&[]);
    e.set_direct(true);
    for ch in ['a', 'k', 'l'] {
        let out = key(&mut e, ch);
        assert!(!out.consumed);
        assert!(out.commit.is_none());
    }
    assert!(!e.handle_key(KeyEvent::ctrl('j')).consumed);
    assert!(!e.handle_key(KeyEvent::new(Key::Escape)).consumed);
    assert_eq!(e.mode(), InputMode::Direct);
}

#[test]
fn set_direct_toggles_without_keys() {
    let mut e = engine(&[]);
    key(&mut e, 'k');
    let out = e.set_direct(true);
    assert_eq!(e.mode(), InputMode::Direct);
    assert!(out.preedit.text.is_empty());
    e.set_direct(false);
    assert_eq!(e.mode(), InputMode::Hira);
    e.set_direct(false);
    assert_eq!(e.mode(), InputMode::Hira);
}
