use crate::api::money::Centimes;
use crate::domain::compte::Compte;
use crate::domain::transaction::{SensTransaction, Transaction};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
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
