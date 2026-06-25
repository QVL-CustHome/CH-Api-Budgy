use crate::crypto::CryptoService;
use crate::db::Db;
use crate::domain::compte::ProprietaireId;
use crate::domain::consent::{Consent, ConsentId, ConsentStatus, NouveauConsent};
use crate::domain::ports::ecriture::{ConsentsWriteRepository, EcritureError};
use crate::domain::ports::lecture::{ConsentsReadRepository, LectureError};
use crate::repository::chiffrement::{
    ChiffrementError, KEY_VERSION, chiffrer_texte, dechiffrer_texte, vers_ecriture_error,
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

const TABLE: &str = "consent";
const FIELD_EXTERNAL_REF: &str = "external_ref";

#[derive(Clone)]
pub struct SqlxConsentsRepository {
    db: Db,
}

impl SqlxConsentsRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub async fn insert(
        &self,
        crypto: &CryptoService,
        nouveau: NouveauConsent,
    ) -> Result<ConsentId, ChiffrementError> {
        let owner = &nouveau.proprietaire.0;
        let external_ref =
            chiffrer_texte(crypto, owner, TABLE, FIELD_EXTERNAL_REF, &nouveau.external_ref)?;

        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO budgy.consent (owner_id, external_ref, status, expires_at, key_version) \
             VALUES ($1, $2, $3, $4, $5) RETURNING id",
        )
        .bind(owner)
        .bind(external_ref)
        .bind(nouveau.status.as_str())
        .bind(nouveau.expires_at)
        .bind(KEY_VERSION)
        .fetch_one(&self.db)
        .await?;

        Ok(ConsentId(id))
    }

    pub async fn fetch(
        &self,
        crypto: &CryptoService,
        id: &ConsentId,
    ) -> Result<Option<Consent>, ChiffrementError> {
        let Some(row) = sqlx::query_as::<_, ConsentRow>(
            "SELECT id, owner_id, external_ref, status, expires_at, created_at, updated_at \
             FROM budgy.consent WHERE id = $1",
        )
        .bind(id.0)
        .fetch_optional(&self.db)
        .await?
        else {
            return Ok(None);
        };

        Ok(Some(into_consent(crypto, row)?))
    }

    pub async fn lister_actifs_par_proprietaire(
        &self,
        crypto: &CryptoService,
        proprietaire: &ProprietaireId,
    ) -> Result<Vec<Consent>, ChiffrementError> {
        let rows = sqlx::query_as::<_, ConsentRow>(
            "SELECT id, owner_id, external_ref, status, expires_at, created_at, updated_at \
             FROM budgy.consent WHERE owner_id = $1 AND status = $2",
        )
        .bind(&proprietaire.0)
        .bind(ConsentStatus::Active.as_str())
        .fetch_all(&self.db)
        .await?;

        rows.into_iter()
            .map(|row| into_consent(crypto, row))
            .collect()
    }

    pub async fn supprimer_par_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<u64, ChiffrementError> {
        let resultat = sqlx::query("DELETE FROM budgy.consent WHERE owner_id = $1")
            .bind(&proprietaire.0)
            .execute(&self.db)
            .await?;
        Ok(resultat.rows_affected())
    }
}

#[derive(Clone)]
pub struct SqlxConsentsWriteAdapter {
    repo: SqlxConsentsRepository,
    crypto: Arc<CryptoService>,
}

impl SqlxConsentsWriteAdapter {
    pub fn new(db: Db, crypto: Arc<CryptoService>) -> Self {
        Self {
            repo: SqlxConsentsRepository::new(db),
            crypto,
        }
    }
}

impl ConsentsWriteRepository for SqlxConsentsWriteAdapter {
    async fn enregistrer(&self, nouveau: NouveauConsent) -> Result<ConsentId, EcritureError> {
        self.repo
            .insert(&self.crypto, nouveau)
            .await
            .map_err(vers_ecriture_error)
    }

    async fn supprimer_par_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<u64, EcritureError> {
        self.repo
            .supprimer_par_proprietaire(proprietaire)
            .await
            .map_err(vers_ecriture_error)
    }
}

impl ConsentsReadRepository for SqlxConsentsWriteAdapter {
    async fn lister_actifs_par_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<Vec<Consent>, LectureError> {
        self.repo
            .lister_actifs_par_proprietaire(&self.crypto, proprietaire)
            .await
            .map_err(|e| LectureError::Acces(e.to_string()))
    }
}

type ConsentRow = (
    Uuid,
    String,
    Vec<u8>,
    String,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
    DateTime<Utc>,
);

fn into_consent(crypto: &CryptoService, row: ConsentRow) -> Result<Consent, ChiffrementError> {
    let (id, owner_id, external_ref_blob, status, expires_at, created_at, updated_at) = row;

    let external_ref =
        dechiffrer_texte(crypto, &owner_id, TABLE, FIELD_EXTERNAL_REF, &external_ref_blob)?;
    let status = ConsentStatus::parse(&status)
        .ok_or_else(|| ChiffrementError::UnknownEnum(status.clone()))?;

    Ok(Consent {
        id: ConsentId(id),
        proprietaire: ProprietaireId(owner_id),
        external_ref,
        status,
        expires_at,
        created_at,
        updated_at,
    })
}
