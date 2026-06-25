use crate::domain::compte::ProprietaireId;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentStatus {
    Pending,
    Active,
    Expired,
    Revoked,
    Failed,
}

impl ConsentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConsentStatus::Pending => "pending",
            ConsentStatus::Active => "active",
            ConsentStatus::Expired => "expired",
            ConsentStatus::Revoked => "revoked",
            ConsentStatus::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(ConsentStatus::Pending),
            "active" => Some(ConsentStatus::Active),
            "expired" => Some(ConsentStatus::Expired),
            "revoked" => Some(ConsentStatus::Revoked),
            "failed" => Some(ConsentStatus::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Consent {
    pub id: ConsentId,
    pub proprietaire: ProprietaireId,
    pub external_ref: String,
    pub status: ConsentStatus,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NouveauConsent {
    pub proprietaire: ProprietaireId,
    pub external_ref: String,
    pub status: ConsentStatus,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NouveauConsentInitie {
    pub id: ConsentId,
    pub proprietaire: ProprietaireId,
    pub external_ref: String,
    pub status: ConsentStatus,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct MiseAJourConsent {
    pub status: ConsentStatus,
    pub external_ref: String,
    pub expires_at: Option<DateTime<Utc>>,
}
