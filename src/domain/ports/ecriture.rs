use crate::domain::balance::{BalanceId, NouvelleBalance};
use crate::domain::bank_account::{BankAccountId, NouveauBankAccount};
use crate::domain::consent::{ConsentId, NouveauConsent};
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
}

pub trait BankAccountsWriteRepository: Send + Sync {
    fn enregistrer(
        &self,
        nouveau: NouveauBankAccount,
    ) -> impl Future<Output = Result<BankAccountId, EcritureError>> + Send;
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
}
