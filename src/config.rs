use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use figment::Figment;
use figment::providers::{Env, Format, Toml};
use serde::Deserialize;

pub const ENCRYPTION_KEY_BYTES: usize = 32;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("fichier de configuration invalide : {0}")]
    File(Box<figment::Error>),
    #[error("variable d'environnement requise manquante ou vide : {0}")]
    MissingSecret(&'static str),
    #[error("BUDGY_ENCRYPTION_KEY invalide : attendu {ENCRYPTION_KEY_BYTES} octets en base64")]
    InvalidEncryptionKey,
}

impl From<figment::Error> for ConfigError {
    fn from(e: figment::Error) -> Self {
        ConfigError::File(Box::new(e))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

#[derive(Clone)]
pub struct Secrets {
    pub database_url: String,
    pub encryption_key: Vec<u8>,
}

impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secrets").finish_non_exhaustive()
    }
}

pub struct Settings {
    pub config: Config,
    pub secrets: Secrets,
}

pub fn load(path: &str) -> Result<Settings, ConfigError> {
    let mut config: Config = Figment::new()
        .merge(Toml::file(path))
        .merge(Env::prefixed("CH__").split("__"))
        .extract()?;

    if let Some(port) = optional("PORT").and_then(|p| p.parse::<u16>().ok()) {
        config.server.port = port;
    }

    let secrets = Secrets {
        database_url: require("DATABASE_URL")?,
        encryption_key: decode_encryption_key(&require("BUDGY_ENCRYPTION_KEY")?)?,
    };

    Ok(Settings { config, secrets })
}

fn decode_encryption_key(value: &str) -> Result<Vec<u8>, ConfigError> {
    let key = STANDARD
        .decode(value.trim())
        .map_err(|_| ConfigError::InvalidEncryptionKey)?;
    if key.len() != ENCRYPTION_KEY_BYTES {
        return Err(ConfigError::InvalidEncryptionKey);
    }
    Ok(key)
}

fn require(name: &'static str) -> Result<String, ConfigError> {
    optional(name).ok_or(ConfigError::MissingSecret(name))
}

fn optional(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn default_log_level() -> String {
    "INFO".to_string()
}
