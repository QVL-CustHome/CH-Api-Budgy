use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompteId(pub Uuid);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProprietaireId(pub String);

#[derive(Debug, Clone)]
pub struct Compte {
    pub id: CompteId,
    pub proprietaire: ProprietaireId,
    pub libelle: String,
    pub etablissement: String,
    pub iban: Option<String>,
    pub devise: String,
    pub solde_centimes: i64,
    pub cree_le: DateTime<Utc>,
    pub mis_a_jour_le: DateTime<Utc>,
}
