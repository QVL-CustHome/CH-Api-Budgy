use crate::db::Db;
use crate::domain::budget::{Budget, BudgetId, NouveauBudget};
use crate::domain::category::CategoryId;
use crate::domain::compte::ProprietaireId;
use crate::domain::ports::ecriture::{BudgetsWriteRepository, EcritureError};
use crate::domain::ports::lecture::{BudgetsReadRepository, LectureError};
use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

#[derive(Clone)]
pub struct SqlxBudgetsRepository {
    db: Db,
}

impl SqlxBudgetsRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

impl BudgetsWriteRepository for SqlxBudgetsRepository {
    async fn enregistrer(&self, nouveau: NouveauBudget) -> Result<Option<Budget>, EcritureError> {
        let row = sqlx::query_as::<_, BudgetRow>(
            "INSERT INTO budgy.budgets (owner_id, category_id, montant_prevu_cents, mois) \
             SELECT $1, c.id, $3, $4 \
             FROM budgy.category c \
             WHERE c.id = $2 AND (c.owner_id = $1 OR c.owner_id IS NULL) \
             ON CONFLICT (owner_id, category_id, mois) \
             DO UPDATE SET montant_prevu_cents = EXCLUDED.montant_prevu_cents, updated_at = now() \
             RETURNING id, owner_id, category_id, montant_prevu_cents, mois, created_at, updated_at",
        )
        .bind(&nouveau.proprietaire.0)
        .bind(nouveau.category_id.0)
        .bind(nouveau.montant_prevu.centimes())
        .bind(nouveau.mois.premier_jour())
        .fetch_optional(&self.db)
        .await
        .map_err(|e| EcritureError::Acces(e.to_string()))?;

        Ok(row.map(into_budget))
    }
}

impl BudgetsReadRepository for SqlxBudgetsRepository {
    async fn lister_par_mois(
        &self,
        proprietaire: &ProprietaireId,
        mois: NaiveDate,
    ) -> Result<Vec<Budget>, LectureError> {
        let rows = sqlx::query_as::<_, BudgetRow>(
            "SELECT id, owner_id, category_id, montant_prevu_cents, mois, created_at, updated_at \
             FROM budgy.budgets \
             WHERE owner_id = $1 AND mois = $2 \
             ORDER BY created_at, id",
        )
        .bind(&proprietaire.0)
        .bind(mois)
        .fetch_all(&self.db)
        .await
        .map_err(|e| LectureError::Acces(e.to_string()))?;

        Ok(rows.into_iter().map(into_budget).collect())
    }
}

type BudgetRow = (
    Uuid,
    String,
    Uuid,
    i64,
    NaiveDate,
    DateTime<Utc>,
    DateTime<Utc>,
);

fn into_budget(row: BudgetRow) -> Budget {
    let (id, owner_id, category_id, montant_prevu_cents, mois, created_at, updated_at) = row;
    Budget {
        id: BudgetId(id),
        owner_id: ProprietaireId(owner_id),
        category_id: CategoryId(category_id),
        montant_prevu_cents,
        mois,
        created_at,
        updated_at,
    }
}
