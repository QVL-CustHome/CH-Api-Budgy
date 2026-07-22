use crate::db::Db;
use crate::domain::category::{
    Category, CategoryId, CategoryKind, MiseAJourCategorie, NouvelleCategorie,
};
use crate::domain::compte::ProprietaireId;
use crate::domain::ports::ecriture::{CategoriesWriteRepository, EcritureError};
use crate::domain::ports::lecture::{
    CategorieAvecCompteur, CategoriesReadRepository, LectureError,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone)]
pub struct SqlxCategoriesRepository {
    db: Db,
}

impl SqlxCategoriesRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

impl CategoriesReadRepository for SqlxCategoriesRepository {
    async fn lister_pour_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<Vec<CategorieAvecCompteur>, LectureError> {
        let rows = sqlx::query_as::<_, CategoryWithCountRow>(
            "SELECT c.id, c.owner_id, c.name, c.kind, c.color, c.icon, c.created_at, \
             (SELECT COUNT(*) FROM budgy.bank_transaction t WHERE t.category_id = c.id) AS transaction_count \
             FROM budgy.category c \
             WHERE c.owner_id IS NULL OR c.owner_id = $1 \
             ORDER BY c.kind, c.name",
        )
        .bind(&proprietaire.0)
        .fetch_all(&self.db)
        .await
        .map_err(|e| LectureError::Acces(e.to_string()))?;

        rows.into_iter().map(into_categorie_avec_compteur).collect()
    }
}

impl CategoriesWriteRepository for SqlxCategoriesRepository {
    async fn creer(&self, nouvelle: NouvelleCategorie) -> Result<Category, EcritureError> {
        let row = sqlx::query_as::<_, CategoryRow>(
            "INSERT INTO budgy.category (owner_id, name, kind, color, icon) \
             VALUES ($1, $2, $3, $4, $5) \
             RETURNING id, owner_id, name, kind, color, icon, created_at",
        )
        .bind(&nouvelle.proprietaire.0)
        .bind(nouvelle.name.as_str())
        .bind(nouvelle.kind.as_str())
        .bind(&nouvelle.color)
        .bind(&nouvelle.icon)
        .fetch_one(&self.db)
        .await
        .map_err(|e| EcritureError::Acces(e.to_string()))?;

        into_category(row).map_err(vers_ecriture_error)
    }

    async fn mettre_a_jour(
        &self,
        proprietaire: &ProprietaireId,
        id: &CategoryId,
        mise_a_jour: MiseAJourCategorie,
    ) -> Result<Option<Category>, EcritureError> {
        let row = sqlx::query_as::<_, CategoryRow>(
            "UPDATE budgy.category SET name = $1, kind = $2, color = $3, icon = $4 \
             WHERE id = $5 AND owner_id = $6 \
             RETURNING id, owner_id, name, kind, color, icon, created_at",
        )
        .bind(mise_a_jour.name.as_str())
        .bind(mise_a_jour.kind.as_str())
        .bind(&mise_a_jour.color)
        .bind(&mise_a_jour.icon)
        .bind(id.0)
        .bind(&proprietaire.0)
        .fetch_optional(&self.db)
        .await
        .map_err(|e| EcritureError::Acces(e.to_string()))?;

        row.map(into_category)
            .transpose()
            .map_err(vers_ecriture_error)
    }

    async fn supprimer(
        &self,
        proprietaire: &ProprietaireId,
        id: &CategoryId,
    ) -> Result<bool, EcritureError> {
        let resultat = sqlx::query("DELETE FROM budgy.category WHERE id = $1 AND owner_id = $2")
            .bind(id.0)
            .bind(&proprietaire.0)
            .execute(&self.db)
            .await
            .map_err(|e| EcritureError::Acces(e.to_string()))?;

        Ok(resultat.rows_affected() > 0)
    }
}

pub(crate) type CategoryRow = (
    Uuid,
    Option<String>,
    String,
    String,
    String,
    String,
    DateTime<Utc>,
);

type CategoryWithCountRow = (
    Uuid,
    Option<String>,
    String,
    String,
    String,
    String,
    DateTime<Utc>,
    i64,
);

fn into_categorie_avec_compteur(
    row: CategoryWithCountRow,
) -> Result<CategorieAvecCompteur, LectureError> {
    let (id, owner_id, name, kind, color, icon, created_at, transaction_count) = row;
    let category = into_category((id, owner_id, name, kind, color, icon, created_at))?;
    Ok(CategorieAvecCompteur {
        category,
        transaction_count,
    })
}

pub(crate) fn into_category(row: CategoryRow) -> Result<Category, LectureError> {
    let (id, owner_id, name, kind, color, icon, created_at) = row;

    let kind = CategoryKind::parse(&kind)
        .ok_or_else(|| LectureError::Acces(format!("type de catégorie inconnu : {kind}")))?;

    Ok(Category {
        id: CategoryId(id),
        owner_id: owner_id.map(ProprietaireId),
        name,
        kind,
        color,
        icon,
        created_at,
    })
}

fn vers_ecriture_error(error: LectureError) -> EcritureError {
    match error {
        LectureError::Acces(message) => EcritureError::Acces(message),
    }
}
