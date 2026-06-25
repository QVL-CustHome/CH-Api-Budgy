use crate::domain::balance::Balance;
use crate::domain::bank_account::BankAccount;
use crate::domain::compte::ProprietaireId;
use crate::domain::consent::Consent;
use crate::domain::transaction_bancaire::TransactionBancaire;
use async_trait::async_trait;
use chrono::NaiveDate;

#[derive(Debug, thiserror::Error)]
pub enum BankDataSourceError {
    #[error("consentement requis ou expiré")]
    ConsentementInvalide,
    #[error("établissement bancaire indisponible")]
    EtablissementIndisponible,
    #[error("ressource bancaire introuvable")]
    RessourceIntrouvable,
    #[error("source bancaire non configurée")]
    SourceNonConfiguree,
    #[error("réponse de la source illisible : {0}")]
    ReponseInvalide(String),
    #[error("erreur de la source bancaire : {0}")]
    Technique(String),
}

#[derive(Debug, Clone)]
pub struct DemandeConsentement {
    pub proprietaire: ProprietaireId,
    pub etablissement: String,
    pub url_retour: String,
}

#[derive(Debug, Clone)]
pub struct ConsentementInitie {
    pub consent: Consent,
    pub url_autorisation: String,
}

#[derive(Debug, Clone)]
pub struct ReponseAutorisation {
    pub reference_autorisation: String,
    pub code_autorisation: String,
}

#[async_trait]
pub trait BankDataSource: Send + Sync {
    async fn initier_consentement(
        &self,
        demande: DemandeConsentement,
    ) -> Result<ConsentementInitie, BankDataSourceError>;

    async fn completer_consentement(
        &self,
        proprietaire: &ProprietaireId,
        reponse: ReponseAutorisation,
    ) -> Result<Consent, BankDataSourceError>;

    async fn lister_comptes(
        &self,
        consent: &Consent,
    ) -> Result<Vec<BankAccount>, BankDataSourceError>;

    async fn solde(
        &self,
        consent: &Consent,
        compte: &BankAccount,
    ) -> Result<Vec<Balance>, BankDataSourceError>;

    async fn lister_transactions(
        &self,
        consent: &Consent,
        compte: &BankAccount,
        depuis: NaiveDate,
    ) -> Result<Vec<TransactionBancaire>, BankDataSourceError>;

    async fn revoquer_consentement(
        &self,
        consent: &Consent,
    ) -> Result<Consent, BankDataSourceError>;
}
