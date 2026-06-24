use crate::domain::bank_account::BankAccountId;
use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionBancaireId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionStatus {
    Booked,
    Pending,
}

impl TransactionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TransactionStatus::Booked => "booked",
            TransactionStatus::Pending => "pending",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "booked" => Some(TransactionStatus::Booked),
            "pending" => Some(TransactionStatus::Pending),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TransactionBancaire {
    pub id: TransactionBancaireId,
    pub bank_account: BankAccountId,
    pub external_transaction_id: String,
    pub status: TransactionStatus,
    pub label: String,
    pub amount_cents: i64,
    pub currency: String,
    pub booking_date: Option<NaiveDate>,
    pub value_date: Option<NaiveDate>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NouvelleTransactionBancaire {
    pub bank_account: BankAccountId,
    pub external_transaction_id: String,
    pub status: TransactionStatus,
    pub label: String,
    pub amount_cents: i64,
    pub currency: String,
    pub booking_date: Option<NaiveDate>,
    pub value_date: Option<NaiveDate>,
}

pub fn dedup_key(bank_account: &BankAccountId, external_transaction_id: &str) -> String {
    format!("{}:{external_transaction_id}", bank_account.0)
}
