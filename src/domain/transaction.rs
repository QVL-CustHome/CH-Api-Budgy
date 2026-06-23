use crate::domain::categorie::CategorieId;
use crate::domain::compte::CompteId;
use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensTransaction {
    Debit,
    Credit,
}

#[derive(Debug, Clone)]
pub struct Transaction {
    pub id: TransactionId,
    pub compte: CompteId,
    pub libelle: String,
    pub montant_centimes: i64,
    pub sens: SensTransaction,
    pub devise: String,
    pub date_operation: NaiveDate,
    pub date_valeur: Option<NaiveDate>,
    pub categorie: Option<CategorieId>,
    pub reference_externe: Option<String>,
    pub cree_le: DateTime<Utc>,
}
