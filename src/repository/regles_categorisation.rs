use crate::db::Db;
use crate::domain::category::CategoryId;
use crate::domain::compte::ProprietaireId;
use crate::domain::ports::ecriture::{EcritureError, ReglesCategorisationWriteRepository};
use crate::domain::ports::lecture::{LectureError, ReglesCategorisationReadRepository};
use crate::domain::regle_categorisation::{
    NouvelleRegleCategorisation, RegleCategorisation, RegleCategorisationId,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone)]
pub struct SqlxReglesCategorisationRepository {
    db: Db,
}

impl SqlxReglesCategorisationRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

impl ReglesCategorisationWriteRepository for SqlxReglesCategorisationRepository {
    async fn creer(
        &self,
        nouvelle: NouvelleRegleCategorisation,
    ) -> Result<Option<RegleCategorisation>, EcritureError> {
        let row = sqlx::query_as::<_, RegleRow>(
            "INSERT INTO budgy.regles_categorisation (owner_id, label_pattern, category_id, priority) \
             SELECT $1, $2, c.id, $4 \
             FROM budgy.category c \
             WHERE c.id = $3 AND (c.owner_id = $1 OR c.owner_id IS NULL) \
             RETURNING id, owner_id, label_pattern, category_id, priority, created_at",
        )
        .bind(&nouvelle.proprietaire.0)
        .bind(nouvelle.label_pattern.as_str())
        .bind(nouvelle.category_id.0)
        .bind(nouvelle.priority)
        .fetch_optional(&self.db)
        .await
        .map_err(|e| EcritureError::Acces(e.to_string()))?;

        Ok(row.map(into_regle))
    }
}

impl ReglesCategorisationReadRepository for SqlxReglesCategorisationRepository {
    async fn lister_pour_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<Vec<RegleCategorisation>, LectureError> {
        let rows = sqlx::query_as::<_, RegleRow>(
            "SELECT id, owner_id, label_pattern, category_id, priority, created_at \
             FROM budgy.regles_categorisation \
             WHERE owner_id = $1 \
             ORDER BY priority DESC, created_at DESC",
        )
        .bind(&proprietaire.0)
        .fetch_all(&self.db)
        .await
        .map_err(|e| LectureError::Acces(e.to_string()))?;

        Ok(rows.into_iter().map(into_regle).collect())
    }
}

type RegleRow = (Uuid, String, String, Uuid, i32, DateTime<Utc>);

fn into_regle(row: RegleRow) -> RegleCategorisation {
    let (id, owner_id, label_pattern, category_id, priority, created_at) = row;
    RegleCategorisation {
        id: RegleCategorisationId(id),
        owner_id: ProprietaireId(owner_id),
        label_pattern,
        category_id: CategoryId(category_id),
        priority,
        created_at,
    }
}
