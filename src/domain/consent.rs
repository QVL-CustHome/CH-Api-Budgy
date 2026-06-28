use crate::domain::compte::ProprietaireId;
use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

pub const MARGE_RENOUVELLEMENT_JOURS_DEFAUT: i64 = 7;

pub fn marge_renouvellement_defaut() -> Duration {
    Duration::days(MARGE_RENOUVELLEMENT_JOURS_DEFAUT)
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentRenouvellement {
    AJour,
    RenouvellementRequis,
    Expire,
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
    pub etablissement: Option<String>,
    pub status: ConsentStatus,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Consent {
    pub fn renouvellement(
        &self,
        maintenant: DateTime<Utc>,
        marge: Duration,
    ) -> ConsentRenouvellement {
        if matches!(self.status, ConsentStatus::Expired | ConsentStatus::Failed) {
            return ConsentRenouvellement::Expire;
        }
        match self.expires_at {
            Some(expiration) if expiration <= maintenant => ConsentRenouvellement::Expire,
            Some(expiration) if expiration <= maintenant + marge => {
                ConsentRenouvellement::RenouvellementRequis
            }
            _ => ConsentRenouvellement::AJour,
        }
    }

    pub fn est_renouvelable(&self, maintenant: DateTime<Utc>, marge: Duration) -> bool {
        !matches!(
            self.renouvellement(maintenant, marge),
            ConsentRenouvellement::AJour
        )
    }
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
    pub etablissement: String,
    pub status: ConsentStatus,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct MiseAJourConsent {
    pub status: ConsentStatus,
    pub external_ref: String,
    pub expires_at: Option<DateTime<Utc>>,
}
