use crate::config::Settings;
use crate::crypto::CryptoService;
use crate::db::Db;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub crypto: Arc<CryptoService>,
}

impl AppState {
    pub fn new(settings: &Settings, db: Db) -> Self {
        Self {
            db,
            crypto: Arc::new(
                CryptoService::from_key(&settings.secrets.encryption_key)
                    .expect("clé de chiffrement validée à 32 octets au chargement de la config"),
            ),
        }
    }
}
