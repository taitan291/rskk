//! UI and I/O independent SKK input engine.

mod engine;
mod kana;
mod kana_table;
mod number;

pub use engine::{
    Candidate, CandidateWindow, Config, Dictionary, Engine, InputMode, Key, KeyEvent, Output,
    Phase, Preedit, PreeditStyle, StyledRange,
};
pub use kana::{to_hankaku_katakana, to_katakana, to_zenkaku};
pub use kana_table::{KanaTable, KanaTableError};
pub use number::{convert_number_candidate, convert_number_key};
