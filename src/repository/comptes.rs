use crate::db::Db;
use crate::domain::compte::{Compte, CompteId, ProprietaireId};
use crate::domain::ports::lecture::{
    ComptesReadRepository, LectureError, LectureResultat, ListeComptesQuery, OwnerRef,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone)]
pub struct SqlxComptesRepository {
    db: Db,
}

impl SqlxComptesRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

type CompteRow = (
    Uuid,
    String,
    String,
    Option<String>,
    String,
    i64,
    DateTime<Utc>,
    DateTime<Utc>,
);

const COMPTE_COLUMNS: &str =
    "id, label, institution, iban, currency, balance_cents, created_at, updated_at";

fn into_compte(owner: &ProprietaireId, row: CompteRow) -> Compte {
    Compte {
        id: CompteId(row.0),
        proprietaire: owner.clone(),
        libelle: row.1,
        etablissement: row.2,
        iban: row.3,
        devise: row.4,
        solde_centimes: row.5,
        cree_le: row.6,
        mis_a_jour_le: row.7,
    }
}

fn proprietaire_from(owner: &OwnerRef) -> ProprietaireId {
    ProprietaireId(owner.0.clone())
}

impl ComptesReadRepository for SqlxComptesRepository {
    async fn lister(
        &self,
        query: ListeComptesQuery,
    ) -> Result<LectureResultat<Compte>, LectureError> {
        let proprietaire = proprietaire_from(&query.owner);

        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM budgy.account WHERE owner_id = $1")
            .bind(&query.owner.0)
            .fetch_one(&self.db)
            .await
            .map_err(|e| LectureError::Acces(e.to_string()))?;

        let rows = sqlx::query_as::<_, CompteRow>(&format!(
            "SELECT {COMPTE_COLUMNS} FROM budgy.account WHERE owner_id = $1 \
             ORDER BY label ASC LIMIT $2 OFFSET $3"
        ))
        .bind(&query.owner.0)
        .bind(i64::from(query.tranche.limit))
        .bind(i64::from(query.tranche.offset))
        .fetch_all(&self.db)
        .await
        .map_err(|e| LectureError::Acces(e.to_string()))?;

        let elements = rows
            .into_iter()
            .map(|row| into_compte(&proprietaire, row))
            .collect();

        Ok(LectureResultat {
            elements,
            total: total.max(0) as u64,
        })
    }

    async fn solde(
        &self,
        owner: &OwnerRef,
        compte: &CompteId,
    ) -> Result<Option<Compte>, LectureError> {
        let proprietaire = proprietaire_from(owner);

        let row = sqlx::query_as::<_, CompteRow>(&format!(
            "SELECT {COMPTE_COLUMNS} FROM budgy.account WHERE owner_id = $1 AND id = $2"
        ))
        .bind(&owner.0)
        .bind(compte.0)
        .fetch_optional(&self.db)
        .await
        .map_err(|e| LectureError::Acces(e.to_string()))?;

        Ok(row.map(|row| into_compte(&proprietaire, row)))
    }
}
