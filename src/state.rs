use crate::adapters::bank::selection::construire_source;
use crate::config::Settings;
use crate::crypto::CryptoService;
use crate::db::Db;
use crate::domain::ports::bank_data_source::BankDataSource;
use crate::repository::bank_accounts::SqlxBankAccountsWriteAdapter;
use crate::repository::bank_transactions::SqlxBankTransactionsWriteAdapter;
use crate::repository::budgets::SqlxBudgetsRepository;
use crate::repository::categories::SqlxCategoriesRepository;
use crate::repository::consents::SqlxConsentsWriteAdapter;
use crate::repository::regles_categorisation::SqlxReglesCategorisationRepository;
use crate::services::jwt::JwtService;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub crypto: Arc<CryptoService>,
    pub jwt: Arc<JwtService>,
    pub consents: Arc<SqlxConsentsWriteAdapter>,
    pub categories: Arc<SqlxCategoriesRepository>,
    pub budgets: Arc<SqlxBudgetsRepository>,
    pub regles_categorisation: Arc<SqlxReglesCategorisationRepository>,
    pub bank_accounts: Arc<SqlxBankAccountsWriteAdapter>,
    pub bank_transactions: Arc<SqlxBankTransactionsWriteAdapter>,
    pub bank_source: Arc<dyn BankDataSource>,
    pub bank_callback_url: String,
}

impl AppState {
    pub fn new(settings: &Settings, db: Db) -> Self {
        let crypto = Arc::new(
            CryptoService::from_key(&settings.secrets.encryption_key)
                .expect("clé de chiffrement validée à 32 octets au chargement de la config"),
        );
        Self {
            consents: Arc::new(SqlxConsentsWriteAdapter::new(db.clone(), crypto.clone())),
            categories: Arc::new(SqlxCategoriesRepository::new(db.clone())),
            budgets: Arc::new(SqlxBudgetsRepository::new(db.clone())),
            regles_categorisation: Arc::new(SqlxReglesCategorisationRepository::new(db.clone())),
            bank_accounts: Arc::new(SqlxBankAccountsWriteAdapter::new(
                db.clone(),
                crypto.clone(),
            )),
            bank_transactions: Arc::new(SqlxBankTransactionsWriteAdapter::new(
                db.clone(),
                crypto.clone(),
            )),
            bank_source: construire_source(
                settings.config.bank.source,
                &settings.config.bank.enable_banking,
            ),
            bank_callback_url: settings.config.bank.callback_url.clone(),
            db,
            crypto,
            jwt: Arc::new(JwtService::from_secret(
                &settings.secrets.jwt_secret,
                &settings.config.token.issuer,
                &settings.config.token.audience,
            )),
        }
    }
}
