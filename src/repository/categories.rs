use crate::db::Db;
use crate::domain::category::{Category, CategoryId, CategoryKind};
use crate::domain::ports::lecture::{CategoriesReadRepository, LectureError};
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
    async fn lister(&self) -> Result<Vec<Category>, LectureError> {
        let rows = sqlx::query_as::<_, CategoryRow>(
            "SELECT id, name, kind, color, icon, created_at \
             FROM budgy.category ORDER BY kind, name",
        )
        .fetch_all(&self.db)
        .await
        .map_err(|e| LectureError::Acces(e.to_string()))?;

        rows.into_iter().map(into_category).collect()
    }
}

type CategoryRow = (Uuid, String, String, String, String, DateTime<Utc>);

fn into_category(row: CategoryRow) -> Result<Category, LectureError> {
    let (id, name, kind, color, icon, created_at) = row;

    let kind = CategoryKind::parse(&kind)
        .ok_or_else(|| LectureError::Acces(format!("type de catégorie inconnu : {kind}")))?;

    Ok(Category {
        id: CategoryId(id),
        name,
        kind,
        color,
        icon,
        created_at,
    })
}
