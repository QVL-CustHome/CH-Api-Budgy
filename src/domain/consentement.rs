use crate::domain::compte::ProprietaireId;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentementId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatutConsentement {
    EnAttente,
    Actif,
    Expire,
    Revoque,
}

#[derive(Debug, Clone)]
pub struct Consentement {
    pub id: ConsentementId,
    pub proprietaire: ProprietaireId,
    pub etablissement: String,
    pub reference_externe: String,
    pub statut: StatutConsentement,
    pub accorde_le: Option<DateTime<Utc>>,
    pub expire_le: Option<DateTime<Utc>>,
}
