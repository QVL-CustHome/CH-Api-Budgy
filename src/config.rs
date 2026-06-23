use figment::Figment;
use figment::providers::{Env, Format, Toml};
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("fichier de configuration invalide : {0}")]
    File(Box<figment::Error>),
    #[error("variable d'environnement requise manquante ou vide : {0}")]
    MissingSecret(&'static str),
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
    };

    Ok(Settings { config, secrets })
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
