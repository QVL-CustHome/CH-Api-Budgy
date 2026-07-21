use crate::api::money::Centimes;
use crate::domain::balance::{Balance, BalanceType};
use crate::domain::bank_account::BankAccount;
use crate::domain::category::{Category, CategoryKind};
use crate::domain::consent::{Consent, ConsentRenouvellement, ConsentStatus};
use crate::domain::ports::bank_data_source::Etablissement;
use crate::domain::ports::lecture::{CategorieAvecCompteur, CompteAvecSolde};
use crate::domain::transaction_bancaire::{
    CategorizationSource, TransactionBancaire, TransactionStatus,
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
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

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CategoryKindDto {
    Revenu,
    Depense,
}

impl From<CategoryKind> for CategoryKindDto {
    fn from(kind: CategoryKind) -> Self {
        match kind {
            CategoryKind::Revenu => CategoryKindDto::Revenu,
            CategoryKind::Depense => CategoryKindDto::Depense,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryDto {
    pub id: Uuid,
    pub name: String,
    pub kind: CategoryKindDto,
    pub color: String,
    pub icon: String,
    pub is_default: bool,
    pub transaction_count: i64,
    pub created_at: DateTime<Utc>,
}

impl CategoryDto {
    pub fn avec_compteur(category: Category, transaction_count: i64) -> Self {
        Self {
            id: category.id.0,
            is_default: category.est_par_defaut(),
            name: category.name,
            kind: category.kind.into(),
            color: category.color,
            icon: category.icon,
            transaction_count,
            created_at: category.created_at,
        }
    }
}

impl From<CategorieAvecCompteur> for CategoryDto {
    fn from(item: CategorieAvecCompteur) -> Self {
        Self::avec_compteur(item.category, item.transaction_count)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CategoryRequest {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
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

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConsentRenewalDto {
    UpToDate,
    RenewalRequired,
    Expired,
}

impl From<ConsentRenouvellement> for ConsentRenewalDto {
    fn from(renouvellement: ConsentRenouvellement) -> Self {
        match renouvellement {
            ConsentRenouvellement::AJour => ConsentRenewalDto::UpToDate,
            ConsentRenouvellement::RenouvellementRequis => ConsentRenewalDto::RenewalRequired,
            ConsentRenouvellement::Expire => ConsentRenewalDto::Expired,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ConsentDto {
    pub consent_id: Uuid,
    pub status: ConsentStatusDto,
    pub renewal: ConsentRenewalDto,
    pub renewable: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ConsentDto {
    pub fn depuis(consent: Consent, maintenant: DateTime<Utc>, marge: Duration) -> Self {
        let renouvellement = consent.renouvellement(maintenant, marge);
        Self {
            consent_id: consent.id.0,
            status: consent.status.into(),
            renewal: renouvellement.into(),
            renewable: !matches!(renouvellement, ConsentRenouvellement::AJour),
            expires_at: consent.expires_at,
            created_at: consent.created_at,
            updated_at: consent.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RenewConsentResponse {
    pub consent_id: Uuid,
    pub authorization_url: String,
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

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CategorizationSourceDto {
    Manual,
    Rule,
    None,
}

impl From<CategorizationSource> for CategorizationSourceDto {
    fn from(source: CategorizationSource) -> Self {
        match source {
            CategorizationSource::Manual => CategorizationSourceDto::Manual,
            CategorizationSource::Rule => CategorizationSourceDto::Rule,
            CategorizationSource::None => CategorizationSourceDto::None,
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
    pub category_id: Option<Uuid>,
    pub categorization_source: CategorizationSourceDto,
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
            category_id: transaction.category.map(|c| c.0),
            categorization_source: transaction.categorization_source.into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CategorizeTransactionRequest {
    pub category_id: Uuid,
}
