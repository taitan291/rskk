mod config;
mod popup;
mod repeat;
mod wl;

use std::{env, time::Instant};

use anyhow::{Context, Result};
use config::AppConfig;
use skk_core::Engine;
use skk_dict::LayeredDictionary;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    if env::args().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "rskk-wayland — Wayland input-method-v2 SKK\n\nUSAGE: rskk-wayland [--help] [--version]\n\nToggle SKK with Ctrl-J or Super-Space. See README.md for configuration."
        );
        return Ok(());
    }
    if env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("rskk-wayland {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let config = AppConfig::load().context("load rskk configuration")?;
    if config.system_dictionaries.is_empty() {
        warn!("no system dictionary configured; conversions will use the user dictionary only");
    }
    let begin = Instant::now();
    let dictionary = LayeredDictionary::open(
        &config.system_dictionaries,
        config.user_dictionary_path(),
        config.engine.immediately_dictionary_rw,
    )
    .context("load SKK dictionaries")?;
    info!(
        elapsed_ms = begin.elapsed().as_millis(),
        systems = config.system_dictionaries.len(),
        "loaded dictionaries"
    );
    let kana_table = config.load_kana_table().context("load kana tables")?;
    let engine = Engine::with_kana_table(config.engine, kana_table, Box::new(dictionary));
    wl::run(
        engine,
        config.key_repeat_rate_limit,
        config.key_repeat_delay_min,
    )
    .context("run Wayland input method")
}
