use crate::adapters::bank::selection::construire_source;
use crate::config::Settings;
use crate::crypto::CryptoService;
use crate::db::Db;
use crate::domain::ports::bank_data_source::BankDataSource;
use crate::repository::comptes::SqlxComptesRepository;
use crate::repository::transactions::SqlxTransactionsRepository;
use crate::services::jwt::JwtService;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub crypto: Arc<CryptoService>,
    pub jwt: Arc<JwtService>,
    pub comptes: Arc<SqlxComptesRepository>,
    pub transactions: Arc<SqlxTransactionsRepository>,
    pub bank_source: Arc<dyn BankDataSource>,
}

impl AppState {
    pub fn new(settings: &Settings, db: Db) -> Self {
        Self {
            comptes: Arc::new(SqlxComptesRepository::new(db.clone())),
            transactions: Arc::new(SqlxTransactionsRepository::new(db.clone())),
            bank_source: construire_source(settings.config.bank.source),
            db,
            crypto: Arc::new(
                CryptoService::from_key(&settings.secrets.encryption_key)
                    .expect("clé de chiffrement validée à 32 octets au chargement de la config"),
            ),
            jwt: Arc::new(JwtService::from_secret(
                &settings.secrets.jwt_secret,
                &settings.config.token.issuer,
                &settings.config.token.audience,
            )),
        }
    }
}
