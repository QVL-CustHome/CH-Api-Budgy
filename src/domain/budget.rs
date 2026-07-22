use crate::domain::category::CategoryId;
use crate::domain::compte::ProprietaireId;
use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetId(pub Uuid);

#[derive(Debug, thiserror::Error)]
pub enum BudgetValidationError {
    #[error("le montant prévu ne peut pas être négatif")]
    MontantNegatif,
    #[error("le mois doit être au format YYYY-MM")]
    MoisInvalide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MontantPrevu(i64);

impl MontantPrevu {
    pub fn parse(centimes: i64) -> Result<Self, BudgetValidationError> {
        if centimes < 0 {
            return Err(BudgetValidationError::MontantNegatif);
        }
        Ok(Self(centimes))
    }

    pub fn centimes(&self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoisBudget(NaiveDate);

impl MoisBudget {
    pub fn parse(valeur: &str) -> Result<Self, BudgetValidationError> {
        let premier_jour = format!("{}-01", valeur.trim());
        NaiveDate::parse_from_str(&premier_jour, "%Y-%m-%d")
            .map(Self)
            .map_err(|_| BudgetValidationError::MoisInvalide)
    }

    pub fn premier_jour(&self) -> NaiveDate {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct Budget {
    pub id: BudgetId,
    pub owner_id: ProprietaireId,
    pub category_id: CategoryId,
    pub montant_prevu_cents: i64,
    pub mois: NaiveDate,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NouveauBudget {
    pub proprietaire: ProprietaireId,
    pub category_id: CategoryId,
    pub montant_prevu: MontantPrevu,
    pub mois: MoisBudget,
}
