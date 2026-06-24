use crate::crypto::CryptoService;
use crate::db::Db;
use crate::domain::compte::ProprietaireId;
use crate::domain::consent::{Consent, ConsentId, ConsentStatus, NouveauConsent};
use crate::repository::chiffrement::{ChiffrementError, KEY_VERSION, chiffrer_texte, dechiffrer_texte};
use chrono::{DateTime, Utc};
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
