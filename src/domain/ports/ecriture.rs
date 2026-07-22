use crate::domain::balance::{BalanceId, NouvelleBalance};
use crate::domain::bank_account::{BankAccountId, NouveauBankAccount, PlanificationSynchro};
use crate::domain::budget::{Budget, NouveauBudget};
use crate::domain::category::{Category, CategoryId, MiseAJourCategorie, NouvelleCategorie};
use crate::domain::compte::ProprietaireId;
use crate::domain::consent::{
    ConsentId, ConsentStatus, MiseAJourConsent, NouveauConsent, NouveauConsentInitie,
};
use crate::domain::regle_categorisation::{NouvelleRegleCategorisation, RegleCategorisation};
use crate::domain::transaction_bancaire::{NouvelleTransactionBancaire, TransactionBancaireId};
use std::future::Future;

#[derive(Debug, thiserror::Error)]
pub enum EcritureError {
    #[error("erreur d'écriture des données : {0}")]
    Acces(String),
    #[error("protection des données impossible : {0}")]
    Protection(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultatInsertion<T> {
    Inseree(T),
    Doublon,
}

pub trait ConsentsWriteRepository: Send + Sync {
    fn enregistrer(
        &self,
        nouveau: NouveauConsent,
    ) -> impl Future<Output = Result<ConsentId, EcritureError>> + Send;

    fn enregistrer_initie(
        &self,
        nouveau: NouveauConsentInitie,
    ) -> impl Future<Output = Result<ConsentId, EcritureError>> + Send;

    fn mettre_a_jour(
        &self,
        proprietaire: &ProprietaireId,
        id: &ConsentId,
        mise_a_jour: MiseAJourConsent,
    ) -> impl Future<Output = Result<bool, EcritureError>> + Send;

    fn marquer_statut(
        &self,
        proprietaire: &ProprietaireId,
        id: &ConsentId,
        status: ConsentStatus,
    ) -> impl Future<Output = Result<bool, EcritureError>> + Send;

    fn supprimer_par_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
    ) -> impl Future<Output = Result<u64, EcritureError>> + Send;
}

pub trait CategoriesWriteRepository: Send + Sync {
    fn creer(
        &self,
        nouvelle: NouvelleCategorie,
    ) -> impl Future<Output = Result<Category, EcritureError>> + Send;

    fn mettre_a_jour(
        &self,
        proprietaire: &ProprietaireId,
        id: &CategoryId,
        mise_a_jour: MiseAJourCategorie,
    ) -> impl Future<Output = Result<Option<Category>, EcritureError>> + Send;

    fn supprimer(
        &self,
        proprietaire: &ProprietaireId,
        id: &CategoryId,
    ) -> impl Future<Output = Result<bool, EcritureError>> + Send;
}

pub trait ReglesCategorisationWriteRepository: Send + Sync {
    fn creer(
        &self,
        nouvelle: NouvelleRegleCategorisation,
    ) -> impl Future<Output = Result<Option<RegleCategorisation>, EcritureError>> + Send;
}

pub trait BudgetsWriteRepository: Send + Sync {
    fn enregistrer(
        &self,
        nouveau: NouveauBudget,
    ) -> impl Future<Output = Result<Option<Budget>, EcritureError>> + Send;
}

pub trait BankAccountsWriteRepository: Send + Sync {
    fn enregistrer(
        &self,
        nouveau: NouveauBankAccount,
    ) -> impl Future<Output = Result<BankAccountId, EcritureError>> + Send;

    fn supprimer_par_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
    ) -> impl Future<Output = Result<u64, EcritureError>> + Send;
}

pub trait BalancesWriteRepository: Send + Sync {
    fn enregistrer(
        &self,
        nouvelle: NouvelleBalance,
    ) -> impl Future<Output = Result<BalanceId, EcritureError>> + Send;
}

pub trait BankTransactionsWriteRepository: Send + Sync {
    fn enregistrer(
        &self,
        nouvelle: NouvelleTransactionBancaire,
    ) -> impl Future<Output = Result<ResultatInsertion<TransactionBancaireId>, EcritureError>> + Send;

    fn recalculer_recurrences(
        &self,
        proprietaire: &ProprietaireId,
    ) -> impl Future<Output = Result<u64, EcritureError>> + Send;
}

pub trait PlanificationSynchroWriteRepository: Send + Sync {
    fn reserver_creneau(
        &self,
        compte: &BankAccountId,
        plan: PlanificationSynchro,
        quota_journalier: i32,
    ) -> impl Future<Output = Result<bool, EcritureError>> + Send;
}

pub trait ConsentsStatutWriteRepository: Send + Sync {
    fn marquer_statut(
        &self,
        consent: &ConsentId,
        statut: ConsentStatus,
    ) -> impl Future<Output = Result<(), EcritureError>> + Send;
}
