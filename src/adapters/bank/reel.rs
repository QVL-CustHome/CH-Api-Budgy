use crate::domain::balance::Balance;
use crate::domain::bank_account::BankAccount;
use crate::domain::consent::Consent;
use crate::domain::ports::bank_data_source::{
    BankDataSource, BankDataSourceError, DemandeConsentement,
};
use crate::domain::transaction_bancaire::TransactionBancaire;
use async_trait::async_trait;
use chrono::NaiveDate;

#[derive(Default)]
pub struct GoCardlessBankDataSource;

impl GoCardlessBankDataSource {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl BankDataSource for GoCardlessBankDataSource {
    async fn initier_consentement(
        &self,
        _demande: DemandeConsentement,
    ) -> Result<Consent, BankDataSourceError> {
        unimplemented!("adapter GoCardless non branché")
    }

    async fn lister_comptes(
        &self,
        _consent: &Consent,
    ) -> Result<Vec<BankAccount>, BankDataSourceError> {
        unimplemented!("adapter GoCardless non branché")
    }

    async fn solde(
        &self,
        _consent: &Consent,
        _compte: &BankAccount,
    ) -> Result<Vec<Balance>, BankDataSourceError> {
        unimplemented!("adapter GoCardless non branché")
    }

    async fn lister_transactions(
        &self,
        _consent: &Consent,
        _compte: &BankAccount,
        _depuis: NaiveDate,
    ) -> Result<Vec<TransactionBancaire>, BankDataSourceError> {
        unimplemented!("adapter GoCardless non branché")
    }
}
