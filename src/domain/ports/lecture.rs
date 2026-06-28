use crate::domain::balance::Balance;
use crate::domain::bank_account::{BankAccount, BankAccountId, CompteASynchroniser};
use crate::domain::compte::ProprietaireId;
use crate::domain::consent::{Consent, ConsentId};
use crate::domain::transaction_bancaire::TransactionBancaire;
use chrono::{DateTime, Utc};
use std::future::Future;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tranche {
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone)]
pub struct LectureResultat<T> {
    pub elements: Vec<T>,
    pub total: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum LectureError {
    #[error("erreur d'accès aux données : {0}")]
    Acces(String),
}

pub trait ConsentsReadRepository: Send + Sync {
    fn lister_actifs_par_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
    ) -> impl Future<Output = Result<Vec<Consent>, LectureError>> + Send;

    fn lister_par_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
    ) -> impl Future<Output = Result<Vec<Consent>, LectureError>> + Send;

    fn fetch_pour_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
        id: &ConsentId,
    ) -> impl Future<Output = Result<Option<Consent>, LectureError>> + Send;
}

pub trait BankAccountsReadRepository: Send + Sync {
    fn lister_par_consent(
        &self,
        proprietaire: &ProprietaireId,
        consent: &ConsentId,
    ) -> impl Future<Output = Result<Vec<BankAccount>, LectureError>> + Send;
}

#[derive(Debug, Clone)]
pub struct CompteAvecSolde {
    pub compte: BankAccount,
    pub solde: Option<Balance>,
}

pub trait ComptesBancairesReadRepository: Send + Sync {
    fn lister_avec_solde(
        &self,
        proprietaire: &ProprietaireId,
        tranche: Tranche,
    ) -> impl Future<Output = Result<LectureResultat<CompteAvecSolde>, LectureError>> + Send;

    fn fetch_avec_solde(
        &self,
        proprietaire: &ProprietaireId,
        compte: &BankAccountId,
    ) -> impl Future<Output = Result<Option<CompteAvecSolde>, LectureError>> + Send;

    fn appartient_au_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
        compte: &BankAccountId,
    ) -> impl Future<Output = Result<bool, LectureError>> + Send;
}

pub trait TransactionsBancairesReadRepository: Send + Sync {
    fn lister_par_compte(
        &self,
        proprietaire: &ProprietaireId,
        compte: &BankAccountId,
        tranche: Tranche,
    ) -> impl Future<Output = Result<LectureResultat<TransactionBancaire>, LectureError>> + Send;
}

#[derive(Debug, Clone)]
pub struct CompteEcheant {
    pub compte: CompteASynchroniser,
    pub consent: Consent,
}

pub trait PlanificationSynchroReadRepository: Send + Sync {
    fn lister_comptes_echeants(
        &self,
        maintenant: DateTime<Utc>,
        quota_journalier: i32,
    ) -> impl Future<Output = Result<Vec<CompteEcheant>, LectureError>> + Send;
}
