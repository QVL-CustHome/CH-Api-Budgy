use crate::domain::bank_account::BankAccountId;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BalanceType {
    Available,
    Booked,
    Expected,
}

impl BalanceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            BalanceType::Available => "available",
            BalanceType::Booked => "booked",
            BalanceType::Expected => "expected",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "available" => Some(BalanceType::Available),
            "booked" => Some(BalanceType::Booked),
            "expected" => Some(BalanceType::Expected),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Balance {
    pub id: BalanceId,
    pub bank_account: BankAccountId,
    pub balance_type: BalanceType,
    pub amount_cents: i64,
    pub currency: String,
    pub reference_date: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NouvelleBalance {
    pub bank_account: BankAccountId,
    pub balance_type: BalanceType,
    pub amount_cents: i64,
    pub currency: String,
    pub reference_date: DateTime<Utc>,
}
