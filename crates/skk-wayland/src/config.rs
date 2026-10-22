use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use skk_core::{Config as EngineConfig, KanaTable};

#[derive(Deserialize)]
#[serde(default)]
pub struct AppConfig {
    #[serde(flatten)]
    pub engine: EngineConfig,
    pub system_dictionaries: Vec<PathBuf>,
    pub user_dictionary: Option<PathBuf>,
    #[serde(alias = "kana_table_files")]
    pub global_kana_table_files: Vec<PathBuf>,
    pub key_repeat_rate_limit: i32,
    pub key_repeat_delay_min: i32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            engine: EngineConfig::default(),
            system_dictionaries: Vec::new(),
            user_dictionary: None,
            global_kana_table_files: Vec::new(),
            key_repeat_rate_limit: 15,
            key_repeat_delay_min: 500,
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let config_path = env::var_os("RSKK_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
                    .map(|root| root.join("rskk/config.toml"))
                    .unwrap_or_else(|| PathBuf::from("rskk/config.toml"))
            });
        let mut config = if config_path.is_file() {
            let source = fs::read_to_string(&config_path)
                .with_context(|| format!("read config {}", config_path.display()))?;
            toml::from_str(&source)
                .with_context(|| format!("parse config {}", config_path.display()))?
        } else {
            Self::default()
        };

        if let Some(paths) = env::var_os("RSKK_SYSTEM_DICT") {
            config.system_dictionaries = env::split_paths(&paths).collect();
            // On Unix RSKK_SYSTEM_DICT is also accepted in the documented colon form.
            if config.system_dictionaries.len() == 1
                && let Some(value) = paths.to_str()
            {
                config.system_dictionaries = value
                    .split(':')
                    .filter(|part| !part.is_empty())
                    .map(PathBuf::from)
                    .collect();
            }
        }
        if let Some(path) = env::var_os("RSKK_USER_DICT") {
            config.user_dictionary = Some(expand_home(PathBuf::from(path)));
        }
        for path in &mut config.system_dictionaries {
            *path = expand_home(path.clone());
        }
        for path in &mut config.global_kana_table_files {
            *path = expand_home(path.clone());
        }
        if config.engine.show_candidates_count == 0 {
            bail!("show_candidates_count must be greater than zero");
        }
        if config.key_repeat_rate_limit < 0 || config.key_repeat_delay_min < 0 {
            bail!("key_repeat_rate_limit and key_repeat_delay_min must not be negative");
        }
        if config.engine.select_candidate_keys.chars().count() != 7
            || config.engine.select_candidate_keys.contains('x')
        {
            bail!("select_candidate_keys must contain seven characters and must not include x");
        }
        Ok(config)
    }

    pub fn user_dictionary_path(&self) -> PathBuf {
        if let Some(path) = &self.user_dictionary {
            return expand_home(path.clone());
        }
        env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
            .map(|root| root.join("rskk/user-dict"))
            .unwrap_or_else(|| PathBuf::from("rskk-user-dict"))
    }

    pub fn load_kana_table(&self) -> Result<KanaTable> {
        let mut table = KanaTable::standard();
        for path in &self.global_kana_table_files {
            let path = expand_home(path.clone());
            let source = fs::read_to_string(&path)
                .with_context(|| format!("read kana table {}", path.display()))?;
            table
                .overlay_toml(&source, &self.engine.kana_table)
                .with_context(|| format!("parse kana table {}", path.display()))?;
        }
        Ok(table)
    }
}

fn expand_home(path: PathBuf) -> PathBuf {
    if path.starts_with("~")
        && let Some(home) = env::var_os("HOME")
    {
        return PathBuf::from(home).join(path.strip_prefix("~").unwrap_or(&path));
    }
    path
}
