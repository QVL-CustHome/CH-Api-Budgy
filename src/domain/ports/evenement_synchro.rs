use crate::domain::compte::ProprietaireId;
use chrono::{DateTime, Utc};
use std::pin::Pin;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeEvenementSynchro {
    SyncStarted,
    SyncSucceeded,
    SyncFailed,
    AccountTransactions,
    BalanceUpdated,
    ConsentRenewalRequired,
    ConsentExpired,
}

impl TypeEvenementSynchro {
    pub fn as_str(&self) -> &'static str {
        match self {
            TypeEvenementSynchro::SyncStarted => "sync.started",
            TypeEvenementSynchro::SyncSucceeded => "sync.succeeded",
            TypeEvenementSynchro::SyncFailed => "sync.failed",
            TypeEvenementSynchro::AccountTransactions => "account.transactions",
            TypeEvenementSynchro::BalanceUpdated => "account.balance",
            TypeEvenementSynchro::ConsentRenewalRequired => "consent.renewal_required",
            TypeEvenementSynchro::ConsentExpired => "consent.expired",
        }
    }

    pub fn segment_topic(&self) -> &'static str {
        match self {
            TypeEvenementSynchro::SyncStarted => "sync/started",
            TypeEvenementSynchro::SyncSucceeded => "sync/succeeded",
            TypeEvenementSynchro::SyncFailed => "sync/failed",
            TypeEvenementSynchro::AccountTransactions => "account/transactions",
            TypeEvenementSynchro::BalanceUpdated => "account/balance",
            TypeEvenementSynchro::ConsentRenewalRequired => "consent/renewal-required",
            TypeEvenementSynchro::ConsentExpired => "consent/expired",
        }
    }

    pub fn retenu(&self) -> bool {
        matches!(
            self,
            TypeEvenementSynchro::ConsentRenewalRequired | TypeEvenementSynchro::ConsentExpired
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvenementSynchro {
    pub proprietaire: ProprietaireId,
    pub type_evenement: TypeEvenementSynchro,
    pub compte: Option<String>,
    pub count: Option<u64>,
    pub at: DateTime<Utc>,
}

impl EvenementSynchro {
    pub fn sync_started(proprietaire: ProprietaireId, compte: String, at: DateTime<Utc>) -> Self {
        Self {
            proprietaire,
            type_evenement: TypeEvenementSynchro::SyncStarted,
            compte: Some(compte),
            count: None,
            at,
        }
    }

    pub fn sync_succeeded(
        proprietaire: ProprietaireId,
        compte: String,
        transactions: u64,
        at: DateTime<Utc>,
    ) -> Self {
        Self {
            proprietaire,
            type_evenement: TypeEvenementSynchro::SyncSucceeded,
            compte: Some(compte),
            count: Some(transactions),
            at,
        }
    }

    pub fn sync_failed(proprietaire: ProprietaireId, compte: String, at: DateTime<Utc>) -> Self {
        Self {
            proprietaire,
            type_evenement: TypeEvenementSynchro::SyncFailed,
            compte: Some(compte),
            count: None,
            at,
        }
    }

    pub fn account_transactions(
        proprietaire: ProprietaireId,
        compte: String,
        count: u64,
        at: DateTime<Utc>,
    ) -> Self {
        Self {
            proprietaire,
            type_evenement: TypeEvenementSynchro::AccountTransactions,
            compte: Some(compte),
            count: Some(count),
            at,
        }
    }

    pub fn balance_updated(
        proprietaire: ProprietaireId,
        compte: String,
        at: DateTime<Utc>,
    ) -> Self {
        Self {
            proprietaire,
            type_evenement: TypeEvenementSynchro::BalanceUpdated,
            compte: Some(compte),
            count: None,
            at,
        }
    }

    pub fn consent_renewal_required(
        proprietaire: ProprietaireId,
        at: DateTime<Utc>,
    ) -> Self {
        Self {
            proprietaire,
            type_evenement: TypeEvenementSynchro::ConsentRenewalRequired,
            compte: None,
            count: None,
            at,
        }
    }

    pub fn consent_expired(proprietaire: ProprietaireId, at: DateTime<Utc>) -> Self {
        Self {
            proprietaire,
            type_evenement: TypeEvenementSynchro::ConsentExpired,
            compte: None,
            count: None,
            at,
        }
    }
}

pub trait EventPublisher: Send + Sync + 'static {
    fn publier(
        &self,
        evenement: EvenementSynchro,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

pub struct NoopEventPublisher;

impl EventPublisher for NoopEventPublisher {
    fn publier(
        &self,
        _evenement: EvenementSynchro,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async {})
    }
}
