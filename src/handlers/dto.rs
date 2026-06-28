use crate::api::money::Centimes;
use crate::domain::balance::{Balance, BalanceType};
use crate::domain::bank_account::BankAccount;
use crate::domain::consent::{Consent, ConsentStatus};
use crate::domain::ports::bank_data_source::Etablissement;
use crate::domain::ports::lecture::CompteAvecSolde;
use crate::domain::transaction_bancaire::{TransactionBancaire, TransactionStatus};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct BankDto {
    pub id: String,
    pub nom: String,
    pub pays: String,
}

impl From<Etablissement> for BankDto {
    fn from(etablissement: Etablissement) -> Self {
        Self {
            id: etablissement.id,
            nom: etablissement.nom,
            pays: etablissement.pays,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateConsentRequest {
    pub bank_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateConsentResponse {
    pub consent_id: Uuid,
    pub authorization_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConsentCallbackRequest {
    pub code: String,
    pub state: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConsentStatusDto {
    Pending,
    Active,
    Expired,
    Revoked,
    Failed,
}

impl From<ConsentStatus> for ConsentStatusDto {
    fn from(status: ConsentStatus) -> Self {
        match status {
            ConsentStatus::Pending => ConsentStatusDto::Pending,
            ConsentStatus::Active => ConsentStatusDto::Active,
            ConsentStatus::Expired => ConsentStatusDto::Expired,
            ConsentStatus::Revoked => ConsentStatusDto::Revoked,
            ConsentStatus::Failed => ConsentStatusDto::Failed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BankAccountDto {
    pub id: Uuid,
    pub iban_masked: String,
    pub currency: String,
    pub created_at: DateTime<Utc>,
}

impl From<BankAccount> for BankAccountDto {
    fn from(compte: BankAccount) -> Self {
        Self {
            id: compte.id.0,
            iban_masked: compte.iban_masked,
            currency: compte.currency,
            created_at: compte.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ConsentDto {
    pub consent_id: Uuid,
    pub status: ConsentStatusDto,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Consent> for ConsentDto {
    fn from(consent: Consent) -> Self {
        Self {
            consent_id: consent.id.0,
            status: consent.status.into(),
            expires_at: consent.expires_at,
            created_at: consent.created_at,
            updated_at: consent.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ConsentCompletionDto {
    pub consent_id: Uuid,
    pub status: ConsentStatusDto,
    pub comptes: Vec<BankAccountDto>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BalanceTypeDto {
    Available,
    Booked,
    Expected,
}

impl From<BalanceType> for BalanceTypeDto {
    fn from(balance_type: BalanceType) -> Self {
        match balance_type {
            BalanceType::Available => BalanceTypeDto::Available,
            BalanceType::Booked => BalanceTypeDto::Booked,
            BalanceType::Expected => BalanceTypeDto::Expected,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BalanceDto {
    pub amount_cents: Centimes,
    #[serde(rename = "type")]
    pub balance_type: BalanceTypeDto,
    pub at: DateTime<Utc>,
}

impl From<Balance> for BalanceDto {
    fn from(balance: Balance) -> Self {
        Self {
            amount_cents: Centimes(balance.amount_cents),
            balance_type: balance.balance_type.into(),
            at: balance.reference_date,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BankAccountSummaryDto {
    pub id: Uuid,
    pub iban_masked: String,
    pub currency: String,
    pub balance: Option<BalanceDto>,
}

impl From<CompteAvecSolde> for BankAccountSummaryDto {
    fn from(item: CompteAvecSolde) -> Self {
        Self {
            id: item.compte.id.0,
            iban_masked: item.compte.iban_masked,
            currency: item.compte.currency,
            balance: item.solde.map(BalanceDto::from),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransactionStatusDto {
    Booked,
    Pending,
}

impl From<TransactionStatus> for TransactionStatusDto {
    fn from(status: TransactionStatus) -> Self {
        match status {
            TransactionStatus::Booked => TransactionStatusDto::Booked,
            TransactionStatus::Pending => TransactionStatusDto::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BankTransactionDto {
    pub id: Uuid,
    pub label: String,
    pub amount_cents: Centimes,
    pub currency: String,
    pub status: TransactionStatusDto,
    pub booking_date: Option<NaiveDate>,
    pub value_date: Option<NaiveDate>,
}

impl From<TransactionBancaire> for BankTransactionDto {
    fn from(transaction: TransactionBancaire) -> Self {
        Self {
            id: transaction.id.0,
            label: transaction.label,
            amount_cents: Centimes(transaction.amount_cents),
            currency: transaction.currency,
            status: transaction.status.into(),
            booking_date: transaction.booking_date,
            value_date: transaction.value_date,
        }
    }
}
