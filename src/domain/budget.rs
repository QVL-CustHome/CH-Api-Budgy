use crate::domain::categorie::CategorieId;
use chrono::NaiveDate;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetId(pub Uuid);

#[derive(Debug, Clone)]
pub struct Budget {
    pub id: BudgetId,
    pub categorie: CategorieId,
    pub plafond_centimes: i64,
    pub periode_debut: NaiveDate,
    pub periode_fin: NaiveDate,
}
