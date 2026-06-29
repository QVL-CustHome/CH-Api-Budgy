use crate::domain::compte::ProprietaireId;
use crate::domain::consent::ConsentId;
use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BankAccountId(pub Uuid);

#[derive(Debug, Clone)]
pub struct BankAccount {
    pub id: BankAccountId,
    pub proprietaire: ProprietaireId,
    pub consent: ConsentId,
    pub external_account_id: String,
    pub iban_masked: String,
    pub currency: String,
    pub next_sync_at: Option<DateTime<Utc>>,
    pub sync_count_today: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NouveauBankAccount {
    pub proprietaire: ProprietaireId,
    pub consent: ConsentId,
    pub external_account_id: String,
    pub iban: String,
    pub currency: String,
    pub next_sync_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct CompteASynchroniser {
    pub id: BankAccountId,
    pub proprietaire: ProprietaireId,
    pub consent: ConsentId,
    pub external_account_id: String,
    pub currency: String,
    pub sync_count_today: i32,
    pub last_sync_day: Option<NaiveDate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanificationSynchro {
    pub next_sync_at: DateTime<Utc>,
    pub last_sync_day: NaiveDate,
    pub last_sync_at: DateTime<Utc>,
}

pub fn masquer_iban(iban: &str) -> String {
    let compact: String = iban.chars().filter(|c| !c.is_whitespace()).collect();
    let conserves = 4;
    if compact.len() <= conserves {
        return "*".repeat(compact.len());
    }
    let suffixe: String = compact.chars().skip(compact.len() - conserves).collect();
    let masque = "*".repeat(compact.len() - conserves);
    format!("{masque}{suffixe}")
}
