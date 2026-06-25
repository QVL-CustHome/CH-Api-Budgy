use crate::api::money::Centimes;
use crate::domain::bank_account::BankAccount;
use crate::domain::compte::Compte;
use crate::domain::consent::{Consent, ConsentStatus};
use crate::domain::ports::bank_data_source::Etablissement;
use crate::domain::transaction::{SensTransaction, Transaction};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct AccountDto {
    pub id: Uuid,
    pub label: String,
    pub institution: String,
    pub iban: Option<String>,
    pub currency: String,
    pub balance_cents: Centimes,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Compte> for AccountDto {
    fn from(compte: Compte) -> Self {
        Self {
            id: compte.id.0,
            label: compte.libelle,
            institution: compte.etablissement,
            iban: compte.iban,
            currency: compte.devise,
            balance_cents: Centimes(compte.solde_centimes),
            created_at: compte.cree_le,
            updated_at: compte.mis_a_jour_le,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountBalanceDto {
    pub account_id: Uuid,
    pub currency: String,
    pub balance_cents: Centimes,
    pub updated_at: DateTime<Utc>,
}

impl From<Compte> for AccountBalanceDto {
    fn from(compte: Compte) -> Self {
        Self {
            account_id: compte.id.0,
            currency: compte.devise,
            balance_cents: Centimes(compte.solde_centimes),
            updated_at: compte.mis_a_jour_le,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransactionDirection {
    Debit,
    Credit,
}

impl From<SensTransaction> for TransactionDirection {
    fn from(sens: SensTransaction) -> Self {
        match sens {
            SensTransaction::Debit => TransactionDirection::Debit,
            SensTransaction::Credit => TransactionDirection::Credit,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TransactionDto {
    pub id: Uuid,
    pub account_id: Uuid,
    pub label: String,
    pub amount_cents: Centimes,
    pub direction: TransactionDirection,
    pub currency: String,
    pub operation_date: NaiveDate,
    pub value_date: Option<NaiveDate>,
    pub category_id: Option<Uuid>,
    pub external_reference: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<Transaction> for TransactionDto {
    fn from(transaction: Transaction) -> Self {
        Self {
            id: transaction.id.0,
            account_id: transaction.compte.0,
            label: transaction.libelle,
            amount_cents: Centimes(transaction.montant_centimes),
            direction: transaction.sens.into(),
            currency: transaction.devise,
            operation_date: transaction.date_operation,
            value_date: transaction.date_valeur,
            category_id: transaction.categorie.map(|c| c.0),
            external_reference: transaction.reference_externe,
            created_at: transaction.cree_le,
        }
    }
}

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
