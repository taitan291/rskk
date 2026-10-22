//! SKK dictionary loading, lookup, and asynchronous user dictionary persistence.

mod escape;
mod parser;
mod system;
mod user;

pub use parser::{DictionaryIndex, Encoding, ParseError, ParseReport, parse_dictionary};
pub use system::LayeredDictionary;
pub use user::{UserDictionary, write_user_dictionary_atomic};
