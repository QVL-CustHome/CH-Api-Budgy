use crate::adapters::bank::selection::SourceBancaire;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use figment::Figment;
use figment::providers::{Env, Format, Toml};
use serde::Deserialize;

pub const ENCRYPTION_KEY_BYTES: usize = 32;
pub const MIN_JWT_SECRET_BYTES: usize = 32;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("fichier de configuration invalide : {0}")]
    File(Box<figment::Error>),
    #[error("variable d'environnement requise manquante ou vide : {0}")]
    MissingSecret(&'static str),
    #[error("BUDGY_ENCRYPTION_KEY invalide : attendu {ENCRYPTION_KEY_BYTES} octets en base64")]
    InvalidEncryptionKey,
    #[error("JWT_SECRET trop court : {0} octets (minimum {MIN_JWT_SECRET_BYTES})")]
    WeakJwtSecret(usize),
}

impl From<figment::Error> for ConfigError {
    fn from(e: figment::Error) -> Self {
        ConfigError::File(Box::new(e))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    #[serde(default)]
    pub token: TokenConfig,
    #[serde(default)]
    pub bank: BankConfig,
    #[serde(default)]
    pub relay: RelayConfig,
    #[serde(default)]
    pub worker_synchro: WorkerSynchroSettings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkerSynchroSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_worker_interval_secondes")]
    pub interval_secondes: u64,
    #[serde(default = "default_worker_quota_journalier")]
    pub quota_journalier: i32,
    #[serde(default = "default_worker_fenetre_jours")]
    pub fenetre_transactions_jours: i64,
}

impl Default for WorkerSynchroSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secondes: default_worker_interval_secondes(),
            quota_journalier: default_worker_quota_journalier(),
            fenetre_transactions_jours: default_worker_fenetre_jours(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RelayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_relay_url")]
    pub url: String,
    #[serde(default = "default_relay_client_id")]
    pub client_id: String,
    #[serde(default = "default_relay_topic_user_deleted")]
    pub topic_user_deleted: String,
    #[serde(default = "default_relay_topic_prefix")]
    pub topic_prefix: String,
    #[serde(default = "default_relay_event_issuer")]
    pub event_issuer: String,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: default_relay_url(),
            client_id: default_relay_client_id(),
            topic_user_deleted: default_relay_topic_user_deleted(),
            topic_prefix: default_relay_topic_prefix(),
            event_issuer: default_relay_event_issuer(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BankConfig {
    #[serde(default)]
    pub source: SourceBancaire,
    #[serde(default)]
    pub enable_banking: EnableBankingConfig,
    #[serde(default = "default_bank_callback_url")]
    pub callback_url: String,
}

impl Default for BankConfig {
    fn default() -> Self {
        Self {
            source: SourceBancaire::default(),
            enable_banking: EnableBankingConfig::default(),
            callback_url: default_bank_callback_url(),
        }
    }
}

#[derive(Clone, Deserialize)]
pub struct EnableBankingConfig {
    #[serde(default = "default_enable_banking_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub private_key_pem: Option<String>,
    #[serde(default)]
    pub private_key_path: Option<String>,
    #[serde(default)]
    pub redirect_url: Option<String>,
}

impl Default for EnableBankingConfig {
    fn default() -> Self {
        Self {
            base_url: default_enable_banking_base_url(),
            app_id: None,
            private_key_pem: None,
            private_key_path: None,
            redirect_url: None,
        }
    }
}

impl std::fmt::Debug for EnableBankingConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnableBankingConfig")
            .field("base_url", &self.base_url)
            .field("app_id", &self.app_id.as_ref().map(|_| "***"))
            .field("private_key_pem", &self.private_key_pem.as_ref().map(|_| "***"))
            .field("private_key_path", &self.private_key_path)
            .field("redirect_url", &self.redirect_url)
            .finish()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenConfig {
    #[serde(default = "default_jwt_issuer")]
    pub issuer: String,

    #[serde(default = "default_audience")]
    pub audience: String,
}

impl Default for TokenConfig {
    fn default() -> Self {
        Self {
            issuer: default_jwt_issuer(),
            audience: default_audience(),
        }
    }
}

#[derive(Clone)]
pub struct Secrets {
    pub database_url: String,
    pub encryption_key: Vec<u8>,
    pub jwt_secret: String,
    pub relay_token: Option<String>,
    pub relay_jwt_private_key: Option<String>,
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

    if let Some(issuer) = optional("JWT_ISSUER") {
        config.token.issuer = issuer;
    }

    if let Some(audience) = optional("JWT_AUDIENCE") {
        config.token.audience = audience;
    }

    if let Some(source) = optional("BANK_SOURCE").and_then(|v| parse_source_bancaire(&v)) {
        config.bank.source = source;
    }

    if let Some(callback_url) = optional("BANK_CALLBACK_URL") {
        config.bank.callback_url = callback_url;
    }

    appliquer_overrides_enable_banking(&mut config.bank.enable_banking);
    appliquer_overrides_relay(&mut config.relay);
    appliquer_overrides_worker(&mut config.worker_synchro);

    let secrets = Secrets {
        database_url: require("DATABASE_URL")?,
        encryption_key: decode_encryption_key(&require("BUDGY_ENCRYPTION_KEY")?)?,
        jwt_secret: require("JWT_SECRET")?,
        relay_token: optional("RELAY_SERVICE_TOKEN"),
        relay_jwt_private_key: optional("RELAY_JWT_PRIVATE_KEY"),
    };
    validate_secrets(&secrets)?;

    Ok(Settings { config, secrets })
}

fn validate_secrets(secrets: &Secrets) -> Result<(), ConfigError> {
    let jwt_len = secrets.jwt_secret.len();
    if jwt_len < MIN_JWT_SECRET_BYTES {
        return Err(ConfigError::WeakJwtSecret(jwt_len));
    }
    Ok(())
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

fn parse_source_bancaire(value: &str) -> Option<SourceBancaire> {
    match value.trim().to_lowercase().as_str() {
        "mock" => Some(SourceBancaire::Mock),
        "enablebanking" => Some(SourceBancaire::EnableBanking),
        _ => None,
    }
}

fn appliquer_overrides_enable_banking(enable_banking: &mut EnableBankingConfig) {
    if let Some(base_url) = optional("ENABLE_BANKING_BASE_URL") {
        enable_banking.base_url = base_url;
    }
    if let Some(app_id) = optional("ENABLE_BANKING_APP_ID") {
        enable_banking.app_id = Some(app_id);
    }
    if let Some(pem) = optional("ENABLE_BANKING_PRIVATE_KEY_PEM") {
        enable_banking.private_key_pem = Some(pem);
    }
    if let Some(path) = optional("ENABLE_BANKING_PRIVATE_KEY_PATH") {
        enable_banking.private_key_path = Some(path);
    }
    if let Some(redirect_url) = optional("ENABLE_BANKING_REDIRECT_URL") {
        enable_banking.redirect_url = Some(redirect_url);
    }
}

fn appliquer_overrides_relay(relay: &mut RelayConfig) {
    if let Some(enabled) = optional("RELAY_ENABLED").and_then(|v| parse_bool(&v)) {
        relay.enabled = enabled;
    }
    if let Some(url) = optional("RELAY_URL") {
        relay.url = url;
    }
    if let Some(client_id) = optional("RELAY_CLIENT_ID") {
        relay.client_id = client_id;
    }
    if let Some(topic) = optional("RELAY_TOPIC_USER_DELETED") {
        relay.topic_user_deleted = topic;
    }
    if let Some(prefix) = optional("RELAY_TOPIC_PREFIX") {
        relay.topic_prefix = prefix;
    }
    if let Some(issuer) = optional("RELAY_EVENT_ISSUER") {
        relay.event_issuer = issuer;
    }
}

fn appliquer_overrides_worker(worker: &mut WorkerSynchroSettings) {
    if let Some(enabled) = optional("WORKER_SYNCHRO_ENABLED").and_then(|v| parse_bool(&v)) {
        worker.enabled = enabled;
    }
    if let Some(interval) =
        optional("WORKER_SYNCHRO_INTERVAL_SECONDES").and_then(|v| v.parse::<u64>().ok())
    {
        worker.interval_secondes = interval;
    }
    if let Some(quota) =
        optional("WORKER_SYNCHRO_QUOTA_JOURNALIER").and_then(|v| v.parse::<i32>().ok())
    {
        worker.quota_journalier = quota;
    }
    if let Some(fenetre) =
        optional("WORKER_SYNCHRO_FENETRE_JOURS").and_then(|v| v.parse::<i64>().ok())
    {
        worker.fenetre_transactions_jours = fenetre;
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn default_worker_interval_secondes() -> u64 {
    6 * 60 * 60
}

fn default_worker_quota_journalier() -> i32 {
    4
}

fn default_worker_fenetre_jours() -> i64 {
    30
}

fn default_relay_url() -> String {
    "mqtt://127.0.0.1:1883".to_string()
}

fn default_relay_client_id() -> String {
    "ch-api-budgy".to_string()
}

fn default_relay_topic_user_deleted() -> String {
    "auth/user/deleted".to_string()
}

fn default_relay_topic_prefix() -> String {
    "budgy".to_string()
}

fn default_relay_event_issuer() -> String {
    "ch-api-budgy".to_string()
}

fn default_enable_banking_base_url() -> String {
    "https://api.enablebanking.com".to_string()
}

fn default_bank_callback_url() -> String {
    "https://budgy.custhome.app/banque/callback".to_string()
}

fn default_log_level() -> String {
    "INFO".to_string()
}

fn default_jwt_issuer() -> String {
    "ch-api-authenticator".to_string()
}

fn default_audience() -> String {
    "ch-api-budgy".to_string()
}
