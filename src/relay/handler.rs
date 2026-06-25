use crate::crypto::CryptoService;
use crate::db::Db;
use crate::domain::effacement::EffacementProprietaire;
use crate::domain::ports::bank_data_source::BankDataSource;
use crate::relay::abonne::MessageHandler;
use crate::relay::evenement::parser_user_deleted;
use crate::repository::bank_accounts::SqlxBankAccountsWriteAdapter;
use crate::repository::consents::SqlxConsentsWriteAdapter;
use std::sync::Arc;

#[derive(Clone)]
pub struct UserDeletedHandler {
    db: Db,
    crypto: Arc<CryptoService>,
    bank_source: Arc<dyn BankDataSource>,
}

impl UserDeletedHandler {
    pub fn new(db: Db, crypto: Arc<CryptoService>, bank_source: Arc<dyn BankDataSource>) -> Self {
        Self {
            db,
            crypto,
            bank_source,
        }
    }

    pub async fn appliquer(&self, payload: &[u8]) {
        let proprietaire = match parser_user_deleted(payload) {
            Ok(proprietaire) => proprietaire,
            Err(erreur) => {
                tracing::warn!(cause = %erreur, "événement user.deleted ignoré : payload invalide");
                return;
            }
        };

        let consents = SqlxConsentsWriteAdapter::new(self.db.clone(), self.crypto.clone());
        let comptes = SqlxBankAccountsWriteAdapter::new(self.db.clone(), self.crypto.clone());
        let service = EffacementProprietaire::new(
            consents.clone(),
            consents,
            comptes,
            self.bank_source.clone(),
        );

        match service.effacer_donnees_proprietaire(proprietaire).await {
            Ok(rapport) => {
                tracing::info!(
                    consentements_supprimes = rapport.consentements_supprimes,
                    comptes_supprimes = rapport.comptes_supprimes,
                    revocations_demandees = rapport.revocations_demandees,
                    revocations_echouees = rapport.revocations_echouees,
                    "effacement en cascade du propriétaire terminé"
                );
            }
            Err(erreur) => {
                tracing::error!(cause = %erreur, "effacement en cascade du propriétaire en échec");
            }
        }
    }
}

impl MessageHandler for UserDeletedHandler {
    fn traiter(&self, payload: Vec<u8>) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send>> {
        let handler = self.clone();
        Box::pin(async move {
            handler.appliquer(&payload).await;
        })
    }
}
