use crate::domain::compte::{Compte, CompteId};
use crate::domain::transaction::Transaction;
use chrono::NaiveDate;
use std::future::Future;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerRef(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tranche {
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone)]
pub struct ListeComptesQuery {
    pub owner: OwnerRef,
    pub tranche: Tranche,
}

#[derive(Debug, Clone)]
pub struct ListeTransactionsQuery {
    pub owner: OwnerRef,
    pub compte: Option<CompteId>,
    pub depuis: Option<NaiveDate>,
    pub jusqua: Option<NaiveDate>,
    pub tranche: Tranche,
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

pub trait ComptesReadRepository: Send + Sync {
    fn lister(
        &self,
        query: ListeComptesQuery,
    ) -> impl Future<Output = Result<LectureResultat<Compte>, LectureError>> + Send;

    fn solde(
        &self,
        owner: &OwnerRef,
        compte: &CompteId,
    ) -> impl Future<Output = Result<Option<Compte>, LectureError>> + Send;
}

pub trait TransactionsReadRepository: Send + Sync {
    fn lister(
        &self,
        query: ListeTransactionsQuery,
    ) -> impl Future<Output = Result<LectureResultat<Transaction>, LectureError>> + Send;
}
