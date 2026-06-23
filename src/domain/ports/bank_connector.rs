use crate::domain::compte::{Compte, ProprietaireId};
use crate::domain::consentement::Consentement;
use crate::domain::transaction::Transaction;
use chrono::NaiveDate;
use std::future::Future;

#[derive(Debug, thiserror::Error)]
pub enum BankConnectorError {
    #[error("consentement requis ou expiré")]
    ConsentementInvalide,
    #[error("établissement bancaire indisponible")]
    EtablissementIndisponible,
    #[error("réponse de l'établissement illisible : {0}")]
    ReponseInvalide(String),
    #[error("erreur du connecteur bancaire : {0}")]
    Technique(String),
}

pub struct DemandeConsentement {
    pub proprietaire: ProprietaireId,
    pub etablissement: String,
    pub url_retour: String,
}

pub trait BankConnector: Send + Sync {
    fn initier_consentement(
        &self,
        demande: DemandeConsentement,
    ) -> impl Future<Output = Result<Consentement, BankConnectorError>> + Send;

    fn lister_comptes(
        &self,
        consentement: &Consentement,
    ) -> impl Future<Output = Result<Vec<Compte>, BankConnectorError>> + Send;

    fn lister_transactions(
        &self,
        consentement: &Consentement,
        compte: &Compte,
        depuis: NaiveDate,
    ) -> impl Future<Output = Result<Vec<Transaction>, BankConnectorError>> + Send;
}
