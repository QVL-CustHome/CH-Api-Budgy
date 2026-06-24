use crate::domain::categorie::CategorieId;
use crate::domain::transaction::SensTransaction;
use chrono::NaiveDate;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrevisionnelId(pub Uuid);

#[derive(Debug, Clone)]
pub struct Previsionnel {
    pub id: PrevisionnelId,
    pub libelle: String,
    pub montant_centimes: i64,
    pub sens: SensTransaction,
    pub categorie: Option<CategorieId>,
    pub date_echeance: NaiveDate,
    pub recurrent: bool,
}
